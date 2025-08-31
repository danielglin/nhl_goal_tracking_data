use crate::api_calls::{
    combine_pbp_boxscore_info, get_game_ids_period, get_hometeam_id, get_pbp_data, parse_goal_data,
    save_goal_data, week_or_shorter_period::WeekOrShorterPeriod, Game
};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Days, FixedOffset, NaiveDate, TimeDelta};
use reqwest;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde_json;

mod api_calls;

use std::env;
use std::fmt::Display;
use std::fs::create_dir_all;
use std::fs::File;
use std::io::Write;
use std::ops::Add;
use std::path::Path;
use std::str::FromStr;

fn main() -> Result<()> {
    const NUM_DAYS_ADD_FOR_WK: u64 = 6;
    const NUM_DAYS_IN_WK: u64 = 7;
    const FOLDER: &str = "../data";
    const PBP_BOXSCORE_FILENAME: &str = "pbp_boxscore.json";

    // let season = Season(String::from("20242025"));
    // let game_id = GameId(String::from("2024030156"));
    // let goal_id = GoalId(185);

    let client = Client::new();
    let mut headers = HeaderMap::new();
    headers.insert("Origin", HeaderValue::from_static("https://www.nhl.com"));
    headers.insert("Referer", HeaderValue::from_static("https://www.nhl.com"));
    headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("cross-site"));
    headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36"));
    // let output_path = "../test_data_fake/test_185_2.json";
    // save_goal_data(&client, headers, &season, &game_id, &goal_id, output_path)?;

    // test getting pbp data
    // let pbp_info = get_pbp_data(&client, &game_id)?;
    // println!("pbp_resp: {:?}", pbp_info);

    // test turning pbp resp into goal info
    // let goals = parse_goal_data(pbp_info);
    // println!("goal info: {:?}", goals);

    // // test getting boxscore info
    // let game = Game { id: 2024030156, season: 20242025};
    // let boxscore_info = get_hometeam_id(&client, &game)?;
    // println!("boxscore info: {:?}", boxscore_info);

    // for a game, need to save:
    // 1. the tracking JSON's for each goal
    // 2. the relevant play-by-play details for each goal: event id, scoring team
    // id, and home team defending side
    // 3. the home team id fr the box score

    // go through the dates and break them down into weeks
    // let mut start_date = NaiveDate::from_ymd_opt(2025, 5, 2).expect("Invalid start date");
    // let end_date = NaiveDate::from_ymd_opt(2025, 5, 2).expect("Invalid end date");
    let (mut start_date, end_date) = match parse_date_args() {
        Ok((start, end)) => (start, end),
        Err(e) => {
            // println!("Error when trying to parse the starting and ending dates: {}", e);
            return Err(e);
        }
    };

    while start_date <= end_date {
        // calculate the last day of the week and see if
        // the last day comes before, after, or is the end date
        let last_day_wk = start_date
            .checked_add_days(Days::new(NUM_DAYS_ADD_FOR_WK))
            .expect("Invalid last date of period");
        let period_end_date;

        if last_day_wk < end_date {
            // the period to pull game id's for is the entire wk
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
        // println!("games: {:?}", games);

        for game in &games {
            // // make a folder for the game if necessary
            // // the game folder will live in a folder for a specific day
            // let game_time_utc = format!("{} +0000", &game.startTimeUTC);
            // let game_date = match DateTime::parse_from_str(&game_time_utc, "%Y-%m-%dT%H:%M:%SZ %z")
            // {
            //     Ok(d) => d,
            //     Err(e) => {
            //         println!(
            //             "Error when converting start time of game {} into a date: {}",
            //             game.id, e
            //         );
            //         // start_date = start_date
            //         //     .checked_add_days(Days::new(NUM_DAYS_IN_WK))
            //         //     .expect(&format!("Error when adding days to {}", start_date));
            //         continue;
            //     }
            // };
            // // need to adjust the game time to local time zone to avoid
            // // the date being off
            // let adjusted_game_date = match adjust_to_local_time(game_date, &game.venueUTCOffset) {
            //     Ok(date) => date,
            //     Err(e) => {
            //         println!("Error when adjusting the UTC time to local time for game {}: {}.  Using UTC date/time: {}", game.id, e, game_date);
            //         game_date.date_naive()
            //     }
            // };

            // let game_path = format!(
            //     "{}/{}/{}/{}",
            //     FOLDER, game.season, adjusted_game_date, game.id
            // );
            // match create_dir_all(&game_path) {
            //     Err(e) => {
            //         println!(
            //             "Error when creating path {} for game {}: {}",
            //             game_path, game.id, e
            //         );
            //         // start_date = start_date
            //         //     .checked_add_days(Days::new(NUM_DAYS_IN_WK))
            //         //     .expect(&format!("Error when adding days to {}", start_date));
            //         continue;
            //     }
            //     Ok(_) => (),
            // };

            // // using the game ids, get the boxscore info for each game
            // // from the boxscore, we just need the home team's id
            // let boxscore_info_result = get_hometeam_id(&client, game);
            // let boxscore_info = match boxscore_info_result {
            //     Ok(boxscore_info) => boxscore_info,
            //     Err(e) => {
            //         println!("Error retrieving boxscore info for game {}: {}", game.id, e);
            //         // start_date = start_date
            //         //     .checked_add_days(Days::new(NUM_DAYS_IN_WK))
            //         //     .expect(&format!("Error when adding days to {}", start_date));
            //         continue;
            //     }
            // };

            // // using the game ids, get the pbp info for each game, including the
            // // goal event ids
            // let pbp_result = get_pbp_data(&client, game);
            // let pbp = match pbp_result {
            //     Ok(pbp) => pbp,
            //     Err(e) => {
            //         println!(
            //             "Error retrieving play-by-play info for game {}: {}",
            //             game.id, e
            //         );
            //         // start_date = start_date
            //         //     .checked_add_days(Days::new(NUM_DAYS_IN_WK))
            //         //     .expect(&format!("Error when adding days to {}", start_date));
            //         continue;
            //     }
            // };

            // let goals = parse_goal_data(pbp);

            // // // using the goal ids, save the goal location JSON's for each goal
            // for goal in &goals {
            //     // make path for the goal
            //     let output_path = format!("{}/{}", game_path, goal.event_id);
            //     match save_goal_data(&client, headers.clone(), &game, goal, &output_path) {
            //         Err(e) => {
            //             println!(
            //                 "Error saving goal data for game {}, goal {}, output filepath {}: {}",
            //                 game.id, goal.event_id, output_path, e
            //             );
            //         }
            //         Ok(_) => (),
            //     }
            // }

            // // save other game info, like pbp and boxscore info, together in
            // // one file
            // let pbp_boxscore_info = combine_pbp_boxscore_info(goals, boxscore_info);
            // let pbp_boxscore_string = serde_json::to_string(&pbp_boxscore_info)?;
            // let pbp_boxscore_path = format!("{}/{}", game_path, PBP_BOXSCORE_FILENAME);
            // let mut pbp_boxscore_file = File::create(pbp_boxscore_path).with_context(|| {
            //     format!(
            //         "Failed to write play-by-play/boxscore data for season: {}, game id: {}",
            //         game.season, game.id
            //     )
            // })?;
            // write!(pbp_boxscore_file, "{}", pbp_boxscore_string)?;
        }

        start_date = start_date
            .checked_add_days(Days::new(NUM_DAYS_IN_WK))
            .expect(&format!("Error when adding days to {}", start_date));
    }

    Ok(())
}

/// Saves all goal data for a single game to a specific folder
fn run_game<P>(
    game: &Game, 
    output_folder: P, 
    client: &Client,
    headers: HeaderMap,
) -> Result<()>
where
    P: AsRef<Path> + Display, 
{
    const PBP_BOXSCORE_FILENAME: &str = "pbp_boxscore.json";

    // make a folder for the game if necessary
    // the game folder will live in a folder for a specific day
    let game_time_utc = format!("{} +0000", &game.startTimeUTC);
    let game_date = match DateTime::parse_from_str(&game_time_utc, "%Y-%m-%dT%H:%M:%SZ %z")
    {
        Ok(d) => d,
        Err(e) => {
            // println!(
            //     "Error when converting start time of game {} into a date: {}",
            //     game.id, e
            // );
            // start_date = start_date
            //     .checked_add_days(Days::new(NUM_DAYS_IN_WK))
            //     .expect(&format!("Error when adding days to {}", start_date));
            return Err(anyhow!("Error when converting start time of game {} into a date: {}", game.id, e))
        }
    };
    // need to adjust the game time to local time zone to avoid
    // the date being off
    let adjusted_game_date = match adjust_to_local_time(game_date, &game.venueUTCOffset) {
        Ok(date) => date,
        Err(e) => {
            println!("Error when adjusting the UTC time to local time for game {}: {}.  Using UTC date/time: {}", game.id, e, game_date);
            game_date.date_naive()
        }
    };

    let game_path = format!(
        "{}/{}/{}/{}",
        output_folder, game.season, adjusted_game_date, game.id
    );
    match create_dir_all(&game_path) {
        Err(e) => {
            println!(
                "Error when creating path {} for game {}: {}",
                game_path, game.id, e
            );
            // start_date = start_date
            //     .checked_add_days(Days::new(NUM_DAYS_IN_WK))
            //     .expect(&format!("Error when adding days to {}", start_date));
            return Err(anyhow!("Error when creating path {} for game {}: {}", game_path, game.id, e))
        }
        Ok(_) => (),
    };

    // using the game ids, get the boxscore info for each game
    // from the boxscore, we just need the home team's id
    let boxscore_info_result = get_hometeam_id(&client, game);
    let boxscore_info = match boxscore_info_result {
        Ok(boxscore_info) => boxscore_info,
        Err(e) => {
            println!("Error retrieving boxscore info for game {}: {}", game.id, e);
            // start_date = start_date
            //     .checked_add_days(Days::new(NUM_DAYS_IN_WK))
            //     .expect(&format!("Error when adding days to {}", start_date));
            return Err(anyhow!("Error retrieving boxscore info for game {}: {}", game.id, e))
        }
    };

    // using the game ids, get the pbp info for each game, including the
    // goal event ids
    let pbp_result = get_pbp_data(&client, game);
    let pbp = match pbp_result {
        Ok(pbp) => pbp,
        Err(e) => {
            println!(
                "Error retrieving play-by-play info for game {}: {}",
                game.id, e
            );
            // start_date = start_date
            //     .checked_add_days(Days::new(NUM_DAYS_IN_WK))
            //     .expect(&format!("Error when adding days to {}", start_date));
            return Err(anyhow!("Error retrieving play-by-play info for game {}: {}", game.id, e))
        }
    };

    let goals = parse_goal_data(pbp);

    // // using the goal ids, save the goal location JSON's for each goal
    for goal in &goals {
        // make path for the goal
        let output_path = format!("{}/{}", game_path, goal.event_id);
        match save_goal_data(&client, headers.clone(), &game, goal, &output_path) {
            Err(e) => {
                println!(
                    "Error saving goal data for game {}, goal {}, output filepath {}: {}",
                    game.id, goal.event_id, output_path, e
                );
            }
            Ok(_) => (),
        }
    }

    // save other game info, like pbp and boxscore info, together in
    // one file
    let pbp_boxscore_info = combine_pbp_boxscore_info(goals, boxscore_info);
    let pbp_boxscore_string = serde_json::to_string(&pbp_boxscore_info)?;
    let pbp_boxscore_path = format!("{}/{}", game_path, PBP_BOXSCORE_FILENAME);
    let mut pbp_boxscore_file = File::create(pbp_boxscore_path).with_context(|| {
        format!(
            "Failed to write play-by-play/boxscore data for season: {}, game id: {}",
            game.season, game.id
        )
    })?;
    write!(pbp_boxscore_file, "{}", pbp_boxscore_string)?;
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
fn parse_date_args() -> Result<(NaiveDate, NaiveDate)> {
    let mut dates = vec![];
    for arg in env::args().skip(1) {
        if dates.len() > 1 {
            return Err(anyhow!("Received too many arguments"));
        }
        let date = NaiveDate::parse_from_str(&arg, "%Y-%m-%d")?;
        dates.push(date);
    }

    if dates.len() < 2 {
        return Err(anyhow!("Received too few arguments (need 2 dates)"));
    }
    Ok((dates[0], dates[1]))
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
}
