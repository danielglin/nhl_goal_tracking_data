use crate::api_calls::{GameExportData, GoalDetails};
use crate::api_calls::{
    get_game_ids_period, get_pbp_data, parse_goal_data,
    save_goal_data, week_or_shorter_period::WeekOrShorterPeriod, get_game_info,
    extract_export_game_data
};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Days, FixedOffset, NaiveDate, TimeDelta};
use clap::{Parser};
use reqwest;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde_json;

mod api_calls;

use std::fmt::Display;
use std::fs::create_dir_all;
use std::fs::File;
use std::io::Write;
use std::ops::Add;
use std::path::Path;
use std::str::FromStr;

fn main() -> Result<()> {

    let client = Client::new();
    let mut headers = HeaderMap::new();
    headers.insert("Origin", HeaderValue::from_static("https://www.nhl.com"));
    headers.insert("Referer", HeaderValue::from_static("https://www.nhl.com"));
    headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("cross-site"));
    headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36"));

    let args = Args::parse();

    // use the correct mode as specified by the user's arg
    // one of game/dates exists because the program will exit
    // if one of them is not provided
    match args.mode.game {
        Some(id) => {
            println!("**** Running single game: {id} ****");            
            run_game(&id, args.output, &client, headers)?;
        },
        None => {
            let (start_date, end_date) = args.mode.dates.expect("Invalid dates");
            println!("**** Running period {start_date} to {end_date} ****");
            run_period(start_date, end_date, args.output, &client, headers)?;
        }
    }

    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(flatten)]
    mode: Mode,

    /// folder to save the output to
    #[arg(long)]
    output: String,
}

#[derive(Parser, Debug)]
#[group(required = true, multiple = false)]
struct Mode {

    /// argument to run a single game: needs to be a valid game id
    #[arg(long)]
    game: Option<String>,

    /// argument to run all the games within a period: dates need to be in
    /// "YYYY-MM-DD::YYYY-MM-DD" format
    #[arg(long, value_parser = parse_date_args)]
    dates: Option<(NaiveDate, NaiveDate)>,
}

/// Saves all goal data for a single game to a specific folder, first by trying
/// the landing endpoint and then if that fails, trying the play-by-play
/// endpoint
fn run_game<P>(
    game_id: &str,
    output_folder: P, 
    client: &Client,
    headers: HeaderMap,
) -> Result<()>
where
    P: AsRef<Path> + Display, 
{
    match run_game_landing(&game_id.to_string(), &output_folder, client, headers.clone()) {
        Err(e) => {
            println!("Error when using landing endpoint for game {}: {}.  Trying play-by-plan endpoint.", game_id, e);

            // try using pbp endpoint instead
            match run_game_pbp(&game_id.to_string(), &output_folder, client, headers.clone()) {
                Err(e) => {
                    Err(anyhow!("Error when using play-by-play endpoint for game {}: {}", game_id, e))
                },
                Ok(_) => Ok(())
            }
        },
        Ok(_) => Ok(()),
    }
}
/// Saves all goal data for a single game to a specific folder using the 
/// game landing endpoint
fn run_game_landing<P>(
    game_id: &str,
    output_folder: P, 
    client: &Client,
    headers: HeaderMap,
) -> Result<()>
where
    P: AsRef<Path> + Display, 
{
    // pull the info using the landing endpoint
    let landing_resp = get_game_info(game_id, client)?;

    // make a folder for the game if necessary
    // the game folder will live in a folder for a specific day
    // let game_time_utc = format!("{} +0000", &game.startTimeUTC);
    let game_date = match NaiveDate::parse_from_str(&landing_resp.gameDate, "%Y-%m-%d") {
        Ok(d) => d,
        Err(e) => {
            return Err(anyhow!("Error when converting start time of game {} into a date: {}; date: {}", landing_resp.id, e, &landing_resp.gameDate))
        }
    };

    let game_path = make_game_folder(output_folder, landing_resp.season, &game_date, landing_resp.id)?;

    let game_data = extract_export_game_data(&landing_resp)?;
    save_goals(&game_data.goals, landing_resp.season, landing_resp.id, &game_path, client, headers);

    // save other game info, like pbp and boxscore info, together in
    // one file
    save_game_data(&game_data, &game_path, landing_resp.season, landing_resp.id)?;
    Ok(())
}

/// Saves all the goal JSON's for several days
fn run_period<P>(
    mut start_date: NaiveDate,
    end_date: NaiveDate,
    output_folder: P,
    client: &Client,
    headers: HeaderMap,
) -> Result<()> 
where
    P: AsRef<Path> + Display, 
{
    const NUM_DAYS_ADD_FOR_WK: u64 = 6;
    const NUM_DAYS_IN_WK: u64 = 7;

    while start_date <= end_date {
        // calculate the last day of the week and see if
        // the last day comes before, after, or is the end date
        let last_day_wk = start_date
            .checked_add_days(Days::new(NUM_DAYS_ADD_FOR_WK))
            .expect("Invalid last date of period");
        let period_end_date;

        if last_day_wk < end_date {
            // the period to pull game id's for is the entire week
            period_end_date = last_day_wk;
        } else if last_day_wk >= end_date {
            period_end_date = end_date;
        } else {
            unreachable!();
            //         unreachable branch needed for the compiler
        }

        println!(
            "start_date: {:?}, end date of period: {:?}",
            start_date, period_end_date
        );
        let period_opt = WeekOrShorterPeriod::try_new(start_date, period_end_date);
        let period = match period_opt {
            Ok(period) => period,
            Err(e) => {
                println!("Invalid period: {}", e);
                start_date = start_date
                    .checked_add_days(Days::new(NUM_DAYS_IN_WK))
                    .expect(&format!("Error when adding days to {}.  Skipping period.", start_date));
                continue;
            }
        };

        // get the game ids for the week
        let game_rslt = get_game_ids_period(&client, &period);
        let games = match game_rslt {
            Ok(game_ids) => game_ids,
            Err(e) => {
                println!("Error retrieving game ids from the schedule API endpoint: {}.  Skipping period: {}", e, &period);
                start_date = start_date
                    .checked_add_days(Days::new(NUM_DAYS_IN_WK))
                    .expect(&format!("Error when adding days to {}.  Skipping period {}.", start_date, &period));
                continue;
            }
        };

        for game in &games {
            match run_game(&game.id.to_string(), &output_folder, client, headers.clone()) {
                Err(e) => {
                    println!("Error when trying to save data for game {}: {}", game.id, e);
                    continue;
                },
                Ok(_) => ()
            }
        }

        start_date = start_date
            .checked_add_days(Days::new(NUM_DAYS_IN_WK))
            .expect(&format!("Error when adding days to {}", start_date));
    }
    Ok(())
}

/// Saves a game's goal JSON's using the play-by-play endpoint
fn run_game_pbp<P>(
    game_id: &str,
    output_folder: P, 
    client: &Client,
    headers: HeaderMap,
) -> Result<()>
where
    P: AsRef<Path> + Display, 
{
    // the play-by-play endpoint has all the info needed to pull goal JSON's
    let pbp_info = get_pbp_data(client, game_id)?;
    let game_date = NaiveDate::parse_from_str(&pbp_info.gameDate, "%Y-%m-%d")?;
    let game_path = make_game_folder(output_folder, pbp_info.season, &game_date, pbp_info.id)?;
    let game_id_int = pbp_info.id;
    let season_id = pbp_info.season;

    let game_export_data = parse_goal_data(pbp_info);
    save_goals(&game_export_data.goals, season_id, game_id_int, &game_path, client, headers);
    save_game_data(&game_export_data, &game_path, season_id, game_id_int)?;
    Ok(())
}

/// Adjusts a game's start time in UTC to the local time
/// By using the venue UTC offset given in the schedule API's response
fn adjust_to_local_time(
    start_time_utc: DateTime<FixedOffset>,
    venue_offset: &str,
) -> Result<NaiveDate> {
    // the format of the offset is given as "+hh:mm" or "-hh::mm"
    // so we need to get both parts
    let hours_adj = i64::from_str(&venue_offset[..3])?;
    let minutes_adj = i64::from_str(&venue_offset[4..6])?;
    let total_adj = TimeDelta::try_minutes(hours_adj * 60 + minutes_adj)
        .ok_or(anyhow!("Couldn't create the start time adjustment"))?;

    Ok(start_time_utc.add(total_adj).date_naive())
}

/// Read in the start and end dates to pull data for from the command-line
/// arguments
/// The dates should be in "YYYY-MM-DD" format.
/// Returns an error if the dates are in invalid formats, or if the end date
/// comes before the start date
fn parse_date_args(arg: &str) -> Result<(NaiveDate, NaiveDate)> {
    const DATE_DELIM: &str = "::";

    let mut dates = vec![];
    for date in arg.split(DATE_DELIM) {
        if dates.len() > 1 {
            return Err(anyhow!("Received too many arguments"));
        }
        let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")?;
        dates.push(date);
    }

    if dates.len() < 2 {
        return Err(anyhow!("Received too few dates (need 2 dates)"));
    }

    if dates[1] < dates[0] {
        return Err(anyhow!("Ending date comes before starting date."))
    }
    Ok((dates[0], dates[1]))
}

/// Makes the folder for the game info, if not already made
/// The game folder has the path: folder/game_date/game_id
fn make_game_folder<P>(
    folder: P,
    season: u32,
    game_date: &NaiveDate,
    game_id: u32,
) -> Result<String> 
where
    P: AsRef<Path> + Display,
{

    let game_path = format!(
        "{}/{}/{}",
        folder, game_date, game_id
    );    
    match create_dir_all(&game_path) {
        Err(e) => {
            Err(anyhow!("Error when creating path {} for game {}: {}", game_path, game_id, e))        
        }
        Ok(_) => Ok(game_path),
    }
}

/// Goes through the goals for a game and save the tracking JSON's
fn save_goals(goals: &[GoalDetails], season: u32, game_id: u32, game_path: &str, client: &Client, headers: HeaderMap) {
    for goal in goals {
        // make path for the goal
        let output_path = format!("{}/{}", game_path, goal.event_id);
        match save_goal_data(client, headers.clone(), season, game_id, goal, &output_path) {
            Err(e) => {
                println!(
                    "Error saving goal data for game {}, goal {}, output filepath {}: {}",
                    game_id, goal.event_id, output_path, e
                );
            }
            Ok(_) => (),
        }
    }
}

/// Saves the additional necessary game info: goal event id's, home defending
/// sides for goals, scoring team id's, and the home team id
fn save_game_data(game_data: &GameExportData, game_path: &str, season: u32, game_id: u32) -> Result<()> {
    const PBP_BOXSCORE_FILENAME: &str = "pbp_boxscore.json";

    let pbp_boxscore_string = serde_json::to_string(&game_data)?;
    let pbp_boxscore_path = format!("{}/{}", game_path, PBP_BOXSCORE_FILENAME);
    let mut pbp_boxscore_file = File::create(pbp_boxscore_path).with_context(|| {
        format!(
            "Failed to write play-by-play/boxscore data for season: {}, game id: {}",
            season, game_id
        )
    })?;
    write!(pbp_boxscore_file, "{}", pbp_boxscore_string)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjust_to_local_time_no_offset() {
        let start_time_utc =
            DateTime::parse_from_str("2025-05-03T00:00:00Z +0000", "%Y-%m-%dT%H:%M:%SZ %z")
                .unwrap();
        let offset = "+00:00";
        let adjusted_date = adjust_to_local_time(start_time_utc, offset).unwrap();
        assert_eq!(adjusted_date, NaiveDate::from_ymd_opt(2025, 5, 3).unwrap());
    }

    // test where the offset is negative, but not big enough to change the date
    #[test]
    fn adjust_to_local_time_neg_offset_no_change() {
        let start_time_utc =
            DateTime::parse_from_str("2025-04-30T10:00:00Z +0000", "%Y-%m-%dT%H:%M:%SZ %z")
                .unwrap();
        let offset = "-09:00";
        let adjusted_date = adjust_to_local_time(start_time_utc, offset).unwrap();
        assert_eq!(adjusted_date, NaiveDate::from_ymd_opt(2025, 4, 30).unwrap());
    }

    // test where the offset is negative and big enough to change the date
    #[test]
    fn adjust_to_local_time_neg_offset_change() {
        let start_time_utc =
            DateTime::parse_from_str("2025-05-01T02:00:00Z +0000", "%Y-%m-%dT%H:%M:%SZ %z")
                .unwrap();
        let offset = "-10:00";
        let adjusted_date = adjust_to_local_time(start_time_utc, offset).unwrap();
        assert_eq!(adjusted_date, NaiveDate::from_ymd_opt(2025, 4, 30).unwrap());
    }

    // test where the offset is positive, but not big enough to change the date
    #[test]
    fn adjust_to_local_time_pos_offset_no_change() {
        let start_time_utc =
            DateTime::parse_from_str("1912-10-20T14:00:00Z +0000", "%Y-%m-%dT%H:%M:%SZ %z")
                .unwrap();
        let offset = "+09:00";
        let adjusted_date = adjust_to_local_time(start_time_utc, offset).unwrap();
        assert_eq!(
            adjusted_date,
            NaiveDate::from_ymd_opt(1912, 10, 20).unwrap()
        );
    }

    // test where the offset is positive and big enough to change the date
    #[test]
    fn adjust_to_local_time_pos_offset_change() {
        let start_time_utc =
            DateTime::parse_from_str("1934-12-31T14:00:00Z +0000", "%Y-%m-%dT%H:%M:%SZ %z")
                .unwrap();
        let offset = "+10:30";
        let adjusted_date = adjust_to_local_time(start_time_utc, offset).unwrap();
        assert_eq!(adjusted_date, NaiveDate::from_ymd_opt(1935, 1, 1).unwrap());
    }

    // test where the offset is invalid
    #[test]
    #[should_panic]
    fn adjust_to_local_time_invalid_offset() {
        let start_time_utc =
            DateTime::parse_from_str("1934-12-31T14:00:00Z +0000", "%Y-%m-%dT%H:%M:%SZ %z")
                .unwrap();
        let offset = "";
        let adjusted_date = adjust_to_local_time(start_time_utc, offset).unwrap();
    }

    ////////////////////////////
    //
    // parse_date_args() tests
    //
    ////////////////////////////
    
    // valid dates: 1 day 
    #[test]
    fn parse_date_args_valid_1_day() {
        let (start_date, end_date) = parse_date_args("2021-10-10::2021-10-10").unwrap();
        assert_eq!(start_date, NaiveDate::from_ymd_opt(2021, 10, 10).unwrap());
        assert_eq!(end_date, NaiveDate::from_ymd_opt(2021, 10, 10).unwrap());
    }

    // valid dates: period crosses years
    #[test]
    fn parse_date_args_valid_cross_yrs() {
        let (start_date, end_date) = parse_date_args("2021-12-31::2022-01-22").unwrap();
        assert_eq!(start_date, NaiveDate::from_ymd_opt(2021, 12, 31).unwrap());
        assert_eq!(end_date, NaiveDate::from_ymd_opt(2022, 1, 22).unwrap());
    }

    // invalid dates: end date before start date
    #[test]
    #[should_panic]
    fn parse_date_args_invalid_end_before() {
        let (start_date, end_date) = parse_date_args("1982-04-30::1982-02-22").unwrap();
    }

    // invalid dates: invalid format
    #[test]
    #[should_panic]
    fn parse_date_args_invalid_format() {
        let (start_date, end_date) = parse_date_args("1982-02-01_to_1982-02-22").unwrap();
    }

    // invalid dates: too many dates
    #[test]
    #[should_panic]
    fn parse_date_args_too_many() {
        let (start_date, end_date) = parse_date_args("1982-04-30::1982-05-22::1982-06-22").unwrap();
    }

    // invalid dates: date that doesn't exist
    #[test]
    #[should_panic]
    fn parse_date_args_invalid_date() {
        let (start_date, end_date) = parse_date_args("1983-04-29::1983-04-31").unwrap();
    }
}
