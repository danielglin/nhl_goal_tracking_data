use chrono::NaiveDate;

use reqwest;
use reqwest::blocking::Client;
use reqwest::header::HeaderMap;

use serde::{Deserialize, Serialize};

use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::api_calls::week_or_shorter_period::WeekOrShorterPeriod;

/// Saves the tracking data for a goal to a file
/// This requires headers to get the data from the NHL site.
pub fn save_goal_data<P>(
    client: &Client,
    headers: HeaderMap,
    season: u32,
    game_id: u32,
    goal: &GoalDetails,
    output_path: P,
) -> Result<()>
where
    P: AsRef<Path>,
{
    const LEN_THRESHOLD: usize = 10;

    // get the tracking data
    let api_url = match &goal.ppt_replay_url {
        Some(url) => url.to_string(),
        None => format!(
            "https://wsr.nhle.com/sprites/{}/{}/ev{}.json",
            season, game_id, goal.event_id
        ),
    };
    let resp = client.get(api_url).headers(headers).send()?;
    if resp.status() == 200 {
        let resp_text = resp.text()?;

        // there are rare cases where the response is an empty string or have 
        // no info, so we should print out a warning for that
        if resp_text.len() < LEN_THRESHOLD {
            println!("Empty response string for season: {}, game id: {}, goal id: {}", season, game_id, goal.event_id);
        }
        // save the data to a file
        let mut file = File::create(output_path).with_context(|| {
            format!(
                "Failed to write tracking data for season: {}, game id: {}, goal id: {} to a file",
                season, game_id, goal.event_id
            )
        })?;
        write!(file, "{}", resp_text)?;
        Ok(())
    } else {
        let err_msg = format!(
            "Unable to get data for season: {}, game id: {}, goal id: {}",
            season, game_id, goal.event_id
        );
        Err(anyhow!(err_msg))
    }
}

// ---------------------------------------------
//
// Finding games
//
// ---------------------------------------------

/// Represents the entire response from the schedule endpoint
#[derive(Deserialize, Debug)]
pub struct ScheduleResponse {
    gameWeek: Vec<GameDaySchedule>,
}

/// Represents the games on a specific date in the schedule response
#[derive(Deserialize, Debug)]
pub struct GameDaySchedule {
    date: String,
    games: Vec<Game>,
}

/// Represents a single game id
#[derive(Deserialize, Debug, Clone)]
pub struct Game {
    pub id: u32,
    pub season: u32,            // need the season to get goal location info
    pub startTimeUTC: String,   // used for creating folders for games
    pub venueUTCOffset: String, // used for creating folders for games
}

pub mod week_or_shorter_period {
    use anyhow::{anyhow, Result};
    use chrono::NaiveDate;

    use std::fmt;

    /// Helper struct that's guaranteed to be a week or shorter
    /// Guarantee comes fr only making WeekOrShorterPeriod through the constructor.
    /// Put the struct in its own module to enforce having to use the constructor
    #[derive(Debug)]
    pub struct WeekOrShorterPeriod {
        start_date: NaiveDate,
        end_date: NaiveDate,
    }

    impl WeekOrShorterPeriod {
        pub fn try_new(start_date: NaiveDate, end_date: NaiveDate) -> Result<Self> {
            const VALID_NUM_DAYS_DIFF: i64 = 6;

            // need to check that the end date is 6 days
            // or less after the start date
            let diff = (end_date - start_date).num_days();
            if (diff <= VALID_NUM_DAYS_DIFF) && (diff >= 0) {
                Ok(Self {
                    start_date,
                    end_date,
                })
            } else {
                let err_msg = format!("Invalid start and end dates: {} and {}.  The time period needs to be a week or shorter, but is actually {} days", start_date, end_date, diff+1);
                Err(anyhow!(err_msg))
            }
        }

        pub fn get_start_date(&self) -> String {
            self.start_date.format("%Y-%m-%d").to_string()
        }

        pub fn within(&self, date: &NaiveDate) -> bool {
            (*date >= self.start_date) && (*date <= self.end_date)
        }
    }

    impl fmt::Display for WeekOrShorterPeriod {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{} to {}", self.start_date.format("%Y-%m-%d").to_string(), self.end_date.format("%Y-%m-%d").to_string())
        }
    }
}

// Gets the game ids that fall within a period
// The period should be a week or shorter.
pub fn get_game_ids_period(
    client: &Client,
    week: &WeekOrShorterPeriod,
) -> Result<Vec<Game>> {
    // let client = Client::new();
    let sched_url = format!(
        "https://api-web.nhle.com/v1/schedule/{}",
        week.get_start_date()
    );
    let mut games = vec![];
    let resp = client.get(sched_url).send()?;
    let text = resp.text()?;

    // parse the response
    let sched_resp: ScheduleResponse = serde_json::from_str(&text)?;

    for game_day in &sched_resp.gameWeek {
        // check that the game day falls w/n the period
        let game_date = NaiveDate::parse_from_str(&game_day.date, "%Y-%m-%d")
            .expect(&format!("Invalid date: {}", &game_day.date));
        if week.within(&game_date) {
            for g in &game_day.games {
                games.push(g.clone())
            }
        }
    }
    Ok(games)
}

// structs to parse pbp info
/// the response from the play-by-play endpoint
#[derive(Deserialize, Debug)]
pub struct PbpResponse {
    plays: Vec<Event>,
    pub id: u32, // this is the game id
    pub season: u32,
    homeTeam: Team,
    pub gameDate: String
}

#[derive(Deserialize, Debug)]
pub struct Event {
    eventId: u32,
    homeTeamDefendingSide: String,
    typeDescKey: String,
    pptReplayUrl: Option<String>,
    details: Option<EventDetails>, // details isn't always present
    periodDescriptor: PeriodInfo,
}

/// generic event details for all event types
#[derive(Deserialize, Debug)]
pub struct EventDetails {
    eventOwnerTeamId: Option<u16>,
}

/// period info used in deserialization
#[derive(Deserialize, Debug)]
pub struct PeriodInfo {
    periodType: String,
}

/// represents a side of the ice
/// by the NHL's API
#[derive(Debug, Serialize, Clone, Copy, PartialEq)]
pub enum IceSide {
    Left,
    Right,
}

/// event details for goals specifically
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct GoalDetails {
    pub event_id: u32,
    ppt_replay_url: Option<String>,
    scoring_team_id: u16,
    home_team_defending_side: IceSide,
}

/// helper struct to serialize extra info needed for all the goals in a game
#[derive(Debug, Serialize)]
pub struct PbpBoxscoreInfo {
    goals: Vec<GoalDetails>,
    home_team_id: u16,
}

/// Get the pbp data for a game
pub fn get_pbp_data(client: &Client, game_id: &str) -> Result<PbpResponse> {
    let pbp_url = format!(
        "https://api-web.nhle.com/v1/gamecenter/{}/play-by-play",
        game_id
    );
    let resp = client.get(pbp_url).send()?;

    if resp.status() == 200 {
        let resp_text = resp.text()?;
        let pbp_resp: PbpResponse = serde_json::from_str(&resp_text)?;

        Ok(pbp_resp)
    } else {
        let err_msg = format!("Unable to get play-by-play for game id: {}, response status: {}.", game_id, resp.status());
        Err(anyhow!(err_msg))
    }
}

/// From data returned by the play-by-play API, get just the goal
/// data for a game
pub fn parse_goal_data(mut pbp: PbpResponse) -> GameExportData {
    let mut goals = vec![];

    // first we need to filter the plays to just the non-shootout goals
    pbp.plays
        .retain(|e| (e.typeDescKey == "goal") && (e.periodDescriptor.periodType != "SO"));

    // get the details out of all the goals to create GoalDetails
    for goal_event in pbp.plays {
        let event_id = goal_event.eventId;
        // let ppt_replay_url = goal_event.pptReplayUrl;
        let scoring_team;

        // get the home team's defending side
        let home_team_defending_side = if goal_event.homeTeamDefendingSide == "left" {
            IceSide::Left
        } else if goal_event.homeTeamDefendingSide == "right" {
            IceSide::Right
        } else {
            println!("Invalid side for goal {} in game {}", event_id, pbp.id);
            continue;
        };

        match goal_event.details {
            Some(details) => {
                // get scoring team, if it exists
                match details.eventOwnerTeamId {
                    Some(id) => {
                        scoring_team = id;
                    }
                    None => {
                        println!(
                            "No scoring team id for goal {} in game {}",
                            event_id, pbp.id
                        );
                        continue;
                    }
                };

                // build the goal details to add to the vec
                let goal_details = GoalDetails {
                    event_id,
                    ppt_replay_url: goal_event.pptReplayUrl,
                    scoring_team_id: scoring_team,
                    home_team_defending_side,
                };
                goals.push(goal_details);
            }
            // if we don't have the details for a goal, don't add it to the
            // vec
            None => {
                println!("No details for goal {} in game {}", event_id, pbp.id);
                continue;
            }
        }
    }
    GameExportData { home_team_id: pbp.homeTeam.id, goals: goals }
}

/////////////////////
//
// Getting the boxscore info
//
/////////////////////

#[derive(Deserialize, Debug)]
pub struct BoxscoreResponse {
    homeTeam: Team,
}

#[derive(Debug, Clone, Copy)]
pub struct BoxscoreInfo {
    home_team_id: u16,
}

/// Gets the home team's id for a game using the boxscore endpoint
/// Returns an error if unable to get the boxscore data
pub fn get_hometeam_id(client: &Client, game: &Game) -> Result<BoxscoreInfo> {
    let boxscore_url = format!(
        "https://api-web.nhle.com/v1/gamecenter/{}/boxscore",
        game.id
    );
    let resp = client.get(boxscore_url).send()?;

    if resp.status() == 200 {
        let resp_text = resp.text()?;
        let boxscore_resp: BoxscoreResponse = serde_json::from_str(&resp_text)?;

        // add in the game id so we don't have just play-by-play
        // info without a way to tie back to a game
        let boxscore_info = BoxscoreInfo {
            home_team_id: boxscore_resp.homeTeam.id,
        };
        Ok(boxscore_info)
    } else {
        let err_msg = format!("Unable to get boxscore for game id: {}.", game.id);
        Err(anyhow!(err_msg))
    }
}

/// Combines both the goal details from the play-by-play info and boxscore info for a game into one
/// struct for serialization
/// Returns an error if the game ids between the two don't match (actually, goal details doesn't have game id, so maybe don't return result)
pub fn combine_pbp_boxscore_info(
    pbp: Vec<GoalDetails>,
    boxscore_info: BoxscoreInfo,
) -> PbpBoxscoreInfo {
    PbpBoxscoreInfo {
        goals: pbp,
        home_team_id: boxscore_info.home_team_id,
    }
}

/// struct for the response from the landing endpoint, which has all the info
/// needed to get the goal tracking data for a game
#[derive(Deserialize, Debug)]
pub struct LandingResponse {
    // game info
    pub id: u32,
    pub season: u32,
    pub gameDate: String,
    homeTeam: Team,
    awayTeam: Team,

    // goal info
    summary: Summary
}

#[derive(Deserialize, Debug)]
struct Team {
    id: u16,
}

#[derive(Deserialize, Debug)]
struct Summary {
    scoring: Vec<Period>
}

#[derive(Deserialize, Debug)]
struct Period {
    periodDescriptor: PeriodDetails,
    goals: Vec<GoalInfo>
}

#[derive(Deserialize, Debug)]
struct PeriodDetails {
    periodType: String
}

#[derive(Deserialize, Debug)]
struct GoalInfo {
    eventId: u32,
    pptReplayUrl: Option<String>,
    homeTeamDefendingSide: String,
    isHome: bool
}

/// Use the landing endpoint to get all the necessary information for a game:
/// Game details:
///     - game id
///     - season id
///     - start date
///     - home team id
/// Goal details:
///     - event id
///     - home team defending side
///     - tracking JSON URL
///     - scoring team
pub fn get_game_info(game_id: &str, client: &Client) -> Result<LandingResponse> {
    let landing_url = format!(
        "https://api-web.nhle.com/v1/gamecenter/{}/landing",
        game_id
    );
    let resp = client.get(landing_url).send()?;

    if resp.status() == 200 {
        let resp_text = resp.text()?;
        let landing_resp: LandingResponse = serde_json::from_str(&resp_text)?;

        Ok(landing_resp)
    } else {
        let err_msg = format!("Unable to get landing info for game id: {}, response status: {}.", game_id, resp.status());
        Err(anyhow!(err_msg))
    }
}

#[derive(Serialize, Debug, PartialEq)]
pub struct GameExportData {
    pub goals: Vec<GoalDetails>,
    home_team_id: u16
}

/// From the landing response, get the game and goal data that's needed
/// in addition to the tracking JSON's
pub fn extract_export_game_data(landing_resp: &LandingResponse) -> Result<GameExportData> {
    // have to go through all the fields in the landing response in order to 
    // get to the goal data
    let mut goals = vec![];
    for period in &landing_resp.summary.scoring {
        // TODO: check that the period isn't the shootout
        // don't want to include shootout goals
        if period.periodDescriptor.periodType == "SO" {
            continue;
        }

        for g in &period.goals {
            // need to figure out the scoring team id by looking at if the 
            // home team scored or not, and then getting the corresponding
            // team id
            let scoring_team_id = if g.isHome {
                landing_resp.homeTeam.id
            } else {
                landing_resp.awayTeam.id
            };

            // convert home team ice side from string to enum
            let home_team_defending_side = if g.homeTeamDefendingSide == "left" {
                IceSide::Left
            } else if g.homeTeamDefendingSide == "right" {
                IceSide::Right
            } else {
                return Err(anyhow!("Invalid side for goal {} in game {}", g.eventId, landing_resp.id));
            };

            goals.push(GoalDetails {
                event_id: g.eventId,
                ppt_replay_url: g.pptReplayUrl.clone(),
                scoring_team_id: scoring_team_id,
                home_team_defending_side: home_team_defending_side
            })
        }
    }

    Ok(GameExportData { goals: goals, home_team_id: landing_resp.homeTeam.id })
}

/// Get just the goal data needed to pull the tracking JSON's from the landing
/// response
pub fn extract_goals() {

}

#[cfg(test)]
mod tests {
    use crate::api_calls::week_or_shorter_period::WeekOrShorterPeriod;

    use super::*;

    //////////////////////////////
    // WeekOrShorterPeriod tests
    //////////////////////////////
    #[test]
    fn valid_wosp() {
        let start_date = NaiveDate::from_ymd_opt(2024, 12, 1).unwrap();
        let end_date = NaiveDate::from_ymd_opt(2024, 12, 3).unwrap();

        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
        assert_eq!(
            wosp.get_start_date(),
            start_date.format("%Y-%m-%d").to_string()
        );
        assert!(wosp.within(&NaiveDate::from_ymd_opt(2024, 12, 3).unwrap()));
        assert!(!wosp.within(&NaiveDate::from_ymd_opt(2024, 11, 30).unwrap()));
        assert!(!wosp.within(&NaiveDate::from_ymd_opt(2024, 12, 5).unwrap()));
    }

    // valid WeekOrShorterPeriod: a period of only one day
    #[test]
    fn valid_wosp_one_day() {
        let start_date = NaiveDate::from_ymd_opt(2024, 12, 1).unwrap();
        let end_date = NaiveDate::from_ymd_opt(2024, 12, 1).unwrap();

        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
        assert_eq!(
            wosp.get_start_date(),
            start_date.format("%Y-%m-%d").to_string()
        );
        assert!(wosp.within(&NaiveDate::from_ymd_opt(2024, 12, 1).unwrap()));
        assert!(!wosp.within(&NaiveDate::from_ymd_opt(2024, 11, 30).unwrap()));
        assert!(!wosp.within(&NaiveDate::from_ymd_opt(2024, 12, 2).unwrap()));
    }

    // valid WeekOrShorterPeriod: a period of exactly one week
    #[test]
    fn valid_wosp_one_week() {
        let start_date = NaiveDate::from_ymd_opt(2024, 12, 1).unwrap();
        let end_date = NaiveDate::from_ymd_opt(2024, 12, 7).unwrap();

        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
        assert_eq!(
            wosp.get_start_date(),
            start_date.format("%Y-%m-%d").to_string()
        );
        assert!(wosp.within(&NaiveDate::from_ymd_opt(2024, 12, 1).unwrap()));
        assert!(wosp.within(&NaiveDate::from_ymd_opt(2024, 12, 7).unwrap()));
        assert!(!wosp.within(&NaiveDate::from_ymd_opt(2024, 11, 30).unwrap()));
        assert!(!wosp.within(&NaiveDate::from_ymd_opt(2024, 12, 8).unwrap()));
    }

    // valid WeekOrShorterPeriod: a period that spans across months
    #[test]
    fn valid_wosp_across_mos() {
        let start_date = NaiveDate::from_ymd_opt(2025, 1, 30).unwrap();
        let end_date = NaiveDate::from_ymd_opt(2025, 2, 5).unwrap();

        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
        assert_eq!(
            wosp.get_start_date(),
            start_date.format("%Y-%m-%d").to_string()
        );
        assert!(wosp.within(&NaiveDate::from_ymd_opt(2025, 1, 30).unwrap()));
        assert!(wosp.within(&NaiveDate::from_ymd_opt(2025, 2, 5).unwrap()));
        assert!(!wosp.within(&NaiveDate::from_ymd_opt(2025, 1, 29).unwrap()));
        assert!(!wosp.within(&NaiveDate::from_ymd_opt(2025, 2, 6).unwrap()));
    }

    // valid WeekOrShorterPeriod: a period that spans across years
    #[test]
    fn valid_wosp_across_yrs() {
        let start_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        let end_date = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();

        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
        assert_eq!(
            wosp.get_start_date(),
            start_date.format("%Y-%m-%d").to_string()
        );
        assert!(wosp.within(&NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()));
        assert!(wosp.within(&NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()));
        assert!(!wosp.within(&NaiveDate::from_ymd_opt(2024, 12, 30).unwrap()));
        assert!(!wosp.within(&NaiveDate::from_ymd_opt(2025, 1, 2).unwrap()));
    }

    // invalid WeekOrShorterPeriod: end date comes before the start date
    #[test]
    #[should_panic]
    fn invalid_wosp_end_date_first() {
        let start_date = NaiveDate::from_ymd_opt(2024, 11, 11).unwrap();
        let end_date = NaiveDate::from_ymd_opt(2024, 11, 10).unwrap();

        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
    }

    // invalid WeekOrShorterPeriod: eight days
    #[test]
    #[should_panic]
    fn invalid_wosp_eight_days() {
        let start_date = NaiveDate::from_ymd_opt(2024, 11, 11).unwrap();
        let end_date = NaiveDate::from_ymd_opt(2024, 11, 18).unwrap();

        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
    }

    // invalid WeekOrShorterPeriod: over a month
    #[test]
    #[should_panic]
    fn invalid_wosp_over_mo() {
        let start_date = NaiveDate::from_ymd_opt(2024, 11, 11).unwrap();
        let end_date = NaiveDate::from_ymd_opt(2024, 12, 18).unwrap();

        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
    }

    /////////////////////////////////////////////
    // tests for WeekOrShorterPeriod.within
    /////////////////////////////////////////////

    // the date is within the period: in the middle
    #[test]
    fn within_true_middle() {
        let start_date = NaiveDate::from_ymd_opt(1991, 2, 11).unwrap();
        let end_date = NaiveDate::from_ymd_opt(1991, 2, 16).unwrap();

        let date = NaiveDate::from_ymd_opt(1991, 2, 14).unwrap();
        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
        assert!(wosp.within(&date));
    }

    // the date is within the period: is the same as the start date
    #[test]
    fn within_true_start_date() {
        let start_date = NaiveDate::from_ymd_opt(1982, 10, 1).unwrap();
        let end_date = NaiveDate::from_ymd_opt(1982, 10, 2).unwrap();

        let date = NaiveDate::from_ymd_opt(1982, 10, 1).unwrap();
        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
        assert!(wosp.within(&date));
    }

    // the date is within the period: is the same as the end date
    #[test]
    fn within_true_end_date() {
        let start_date = NaiveDate::from_ymd_opt(1977, 9, 28).unwrap();
        let end_date = NaiveDate::from_ymd_opt(1977, 9, 30).unwrap();

        let date = NaiveDate::from_ymd_opt(1977, 9, 30).unwrap();
        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
        assert!(wosp.within(&date));
    }

    // the date is not within the period: before the period
    #[test]
    fn within_false_before() {
        let start_date = NaiveDate::from_ymd_opt(1940, 6, 14).unwrap();
        let end_date = NaiveDate::from_ymd_opt(1940, 6, 18).unwrap();

        let date = NaiveDate::from_ymd_opt(1940, 6, 13).unwrap();
        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
        assert!(!wosp.within(&date));
    }

    // the date is not within the period: after the period
    #[test]
    fn within_false_after() {
        let start_date = NaiveDate::from_ymd_opt(1930, 12, 25).unwrap();
        let end_date = NaiveDate::from_ymd_opt(1930, 12, 31).unwrap();

        let date = NaiveDate::from_ymd_opt(1931, 1, 1).unwrap();
        let wosp = WeekOrShorterPeriod::try_new(start_date, end_date).unwrap();
        assert!(!wosp.within(&date));
    }

    /////////////////////////////////////
    //
    // combine_pbp_boxscore_info() tests
    //
    /////////////////////////////////////

    #[test]
    fn combine_pbp_boxscore_id_matches() {
        let goals = vec![GoalDetails {
            scoring_team_id: 19,
            event_id: 502,
            home_team_defending_side: IceSide::Left,
            ppt_replay_url: Some(String::from("https://nhl.com")),
        }];
        // let pbp = PbpInfo { game_id: 12, goals};
        let boxscore = BoxscoreInfo { home_team_id: 19 };

        let expected_combined = PbpBoxscoreInfo {
            goals: goals.clone(),
            home_team_id: 19,
        };
        let actual_combined = combine_pbp_boxscore_info(goals, boxscore);
        assert_eq!(expected_combined.home_team_id, actual_combined.home_team_id);
        assert_eq!(expected_combined.goals.len(), actual_combined.goals.len());
        assert_eq!(
            expected_combined.goals[0].event_id,
            actual_combined.goals[0].event_id
        );
        assert_eq!(
            expected_combined.goals[0].home_team_defending_side,
            actual_combined.goals[0].home_team_defending_side
        );
        assert_eq!(
            expected_combined.goals[0].scoring_team_id,
            actual_combined.goals[0].scoring_team_id
        );
        assert_eq!(
            expected_combined.goals[0].ppt_replay_url,
            actual_combined.goals[0].ppt_replay_url
        );
    }

    //////////////////////////////////////
    //
    // parse_goal_data() tests
    //
    //////////////////////////////////////

    // no goals results in an empty vec
    #[test]
    fn parse_goal_data_no_goals() {
        let plays = vec![Event {
            details: Some(EventDetails {
                eventOwnerTeamId: Some(1),
            }),
            eventId: 90,
            homeTeamDefendingSide: String::from("right"),
            pptReplayUrl: Some(String::from("nhl.com")),
            typeDescKey: String::from("shot"),
            periodDescriptor: PeriodInfo {
                periodType: String::from("REG"),
            },
        }];
        let pbp_info = PbpResponse { plays: plays, id: 1, season: 20252025, homeTeam: Team { id: 19 }, gameDate: String::from("2025-05-02") };

        let actual_goal_details = parse_goal_data(pbp_info);

        assert_eq!(actual_goal_details.goals.len(), 0);
    }

    // one goals results in a vec with just that one goal
    #[test]
    fn parse_goal_data_one_goal() {
        let plays = vec![
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 89,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("shot"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 90,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
        ];
        let pbp_info = PbpResponse { plays: plays, id: 1, season: 20252025, homeTeam: Team { id: 19 }, gameDate: String::from("2025-05-02") };

        let actual_goal_details = parse_goal_data(pbp_info);
        let expected_goal_details = vec![GoalDetails {
            event_id: 90,
            home_team_defending_side: IceSide::Right,
            ppt_replay_url: Some(String::from("nhl.com")),
            scoring_team_id: 1,
        }];

        assert_eq!(actual_goal_details.goals, expected_goal_details);
    }

    // several goals results in a vec with just all the goals
    #[test]
    fn parse_goal_data_many_goals() {
        let plays = vec![
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 89,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("shot"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 90,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com/ev90")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 91,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("faceoff"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 92,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: Some(String::from("nhl.com/ev92")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 93,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com/ev93")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
        ];
        let pbp_info = PbpResponse { plays: plays, id: 1, season: 20252025, homeTeam: Team { id: 19 }, gameDate: String::from("2025-05-02") };

        let actual_goal_details = parse_goal_data(pbp_info);
        let expected_goal_details = vec![
            GoalDetails {
                event_id: 90,
                home_team_defending_side: IceSide::Right,
                ppt_replay_url: Some(String::from("nhl.com/ev90")),
                scoring_team_id: 1,
            },
            GoalDetails {
                event_id: 92,
                home_team_defending_side: IceSide::Left,
                ppt_replay_url: Some(String::from("nhl.com/ev92")),
                scoring_team_id: 19,
            },
            GoalDetails {
                event_id: 93,
                home_team_defending_side: IceSide::Right,
                ppt_replay_url: Some(String::from("nhl.com/ev93")),
                scoring_team_id: 1,
            },
        ];

        assert_eq!(actual_goal_details.goals, expected_goal_details);
    }

    // a game with only shootout goals should have no goals
    #[test]
    fn parse_goal_only_shootout() {
        let plays = vec![
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 89,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("shot"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 90,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com/ev90")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("SO"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 91,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("faceoff"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 92,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: Some(String::from("nhl.com/ev92")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("SO"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 93,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com/ev93")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("SO"),
                },
            },
        ];
        let pbp_info = PbpResponse { plays: plays, id: 1, season: 20252025, homeTeam: Team { id: 19 }, gameDate: String::from("2025-05-02") };

        let actual_goal_details = parse_goal_data(pbp_info);
        let expected_goal_details = vec![];

        assert_eq!(actual_goal_details.goals, expected_goal_details);
    }

    // a game with both regular goals and shootout goals should have just
    // the regular goals
    #[test]
    fn parse_goal_regular_shootout() {
        let plays = vec![
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 89,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("shot"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 90,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com/ev90")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("SO"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 91,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("faceoff"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 92,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: Some(String::from("nhl.com/ev92")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("SO"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 93,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com/ev93")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
        ];
        let pbp_info = PbpResponse { plays: plays, id: 1, season: 20252025, homeTeam: Team { id: 19 }, gameDate: String::from("2025-05-02") };

        let actual_goal_details = parse_goal_data(pbp_info);
        let expected_goal_details = vec![GoalDetails {
            event_id: 93,
            home_team_defending_side: IceSide::Right,
            ppt_replay_url: Some(String::from("nhl.com/ev93")),
            scoring_team_id: 1,
        }];

        assert_eq!(actual_goal_details.goals, expected_goal_details);
    }

    // a agme with both regulation goals and an overtime goal
    #[test]
    fn parse_goal_regulation_ot() {
        let plays = vec![
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 89,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("shot"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 90,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com/ev90")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 91,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("faceoff"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 92,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: Some(String::from("nhl.com/ev92")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 93,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com/ev93")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("OT"),
                },
            },
        ];
        let pbp_info = PbpResponse { plays: plays, id: 1, season: 20252025, homeTeam: Team { id: 19 }, gameDate: String::from("2025-05-02") };

        let actual_goal_details = parse_goal_data(pbp_info);
        let expected_goal_details = vec![
            GoalDetails {
                event_id: 90,
                home_team_defending_side: IceSide::Right,
                ppt_replay_url: Some(String::from("nhl.com/ev90")),
                scoring_team_id: 1,
            },
            GoalDetails {
                event_id: 92,
                home_team_defending_side: IceSide::Left,
                ppt_replay_url: Some(String::from("nhl.com/ev92")),
                scoring_team_id: 19,
            },
            GoalDetails {
                event_id: 93,
                home_team_defending_side: IceSide::Right,
                ppt_replay_url: Some(String::from("nhl.com/ev93")),
                scoring_team_id: 1,
            },
        ];

        assert_eq!(actual_goal_details.goals, expected_goal_details);
    }

    // a game where some goals don't have the replay URL
    // need to include those goals too
    #[test]
    fn parse_goal_missing_replay_urls() {
        let plays = vec![
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 89,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("shot"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 90,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com/ev90")),
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 91,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: Some(String::from("nhl.com")),
                typeDescKey: String::from("faceoff"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(19),
                }),
                eventId: 92,
                homeTeamDefendingSide: String::from("left"),
                pptReplayUrl: None,
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("REG"),
                },
            },
            Event {
                details: Some(EventDetails {
                    eventOwnerTeamId: Some(1),
                }),
                eventId: 93,
                homeTeamDefendingSide: String::from("right"),
                pptReplayUrl: None,
                typeDescKey: String::from("goal"),
                periodDescriptor: PeriodInfo {
                    periodType: String::from("OT"),
                },
            },
        ];
        let pbp_info = PbpResponse { plays: plays, id: 1, season: 20252025, homeTeam: Team { id: 19 }, gameDate: String::from("2025-05-02") };

        let actual_goal_details = parse_goal_data(pbp_info);
        let expected_goal_details = vec![
            GoalDetails {
                event_id: 90,
                home_team_defending_side: IceSide::Right,
                ppt_replay_url: Some(String::from("nhl.com/ev90")),
                scoring_team_id: 1,
            },
            GoalDetails {
                event_id: 92,
                home_team_defending_side: IceSide::Left,
                ppt_replay_url: None,
                scoring_team_id: 19,
            },
            GoalDetails {
                event_id: 93,
                home_team_defending_side: IceSide::Right,
                ppt_replay_url: None,
                scoring_team_id: 1,
            },
        ];

        assert_eq!(actual_goal_details.goals, expected_goal_details);
    }

    //////////////////////////////////////////
    //
    // Landing endpoint tests
    //
    //////////////////////////////////////////
    
    // Game with only regulation goals should have all goals 
    #[test]
    fn extract_export_game_data_regl_only() {
        let period_1 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![GoalInfo { eventId: 12, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("right"), isHome: false}]
        };
        let period_2 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![]
        };
        let period_3 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![
                GoalInfo { eventId: 120, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("right"), isHome: false},
                GoalInfo { eventId: 170, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("left"), isHome: true},
            ]
        };
        let summary = Summary { 
            scoring: vec![
                period_1,
                period_2, 
                period_3
            ]
        };
        let landing_resp = LandingResponse { 
            id: 2024000201, season: 20242025, gameDate: String::from("2024-10-29"), 
            homeTeam: Team { id: 10 }, awayTeam: Team { id: 19 }, summary: summary };
        
        let actual_game_export = extract_export_game_data(&landing_resp).unwrap();
        let expected_game_export = GameExportData {
            home_team_id: 10,
            goals: vec![
                GoalDetails {
                    event_id: 12,
                    ppt_replay_url: Some(String::from("nhl.com")),
                    scoring_team_id: 19,
                    home_team_defending_side: IceSide::Right
                },
                GoalDetails {
                    event_id: 120,
                    ppt_replay_url: Some(String::from("nhl.com")),
                    scoring_team_id: 19,
                    home_team_defending_side: IceSide::Right
                },
                GoalDetails {
                    event_id: 170,
                    ppt_replay_url: Some(String::from("nhl.com")),
                    scoring_team_id: 10,
                    home_team_defending_side: IceSide::Left
                },
            ]
        };

        assert_eq!(actual_game_export, expected_game_export);
    }

    // Game with only shootout goals should not have any goals
    #[test]
    fn extract_export_game_data_so_only() {
        let period_1 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![]
        };
        let period_2 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![]
        };
        let period_3 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![]
        };
        let ot = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("OT"),
            },
            goals: vec![]
        };
        let shootout = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("SO"),
            },
            goals: vec![
                GoalInfo { eventId: 486, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("right"), isHome: false}
            ]
        };
        let summary = Summary { 
            scoring: vec![
                period_1,
                period_2, 
                period_3,
                ot,
                shootout
            ]
        };
        let landing_resp = LandingResponse { 
            id: 2024000201, season: 20242025, gameDate: String::from("2024-10-29"), 
            homeTeam: Team { id: 10 }, awayTeam: Team { id: 19 }, summary: summary };
        
        let actual_game_export = extract_export_game_data(&landing_resp).unwrap();
        let expected_game_export = GameExportData {
            home_team_id: 10,
            goals: vec![]
        };

        assert_eq!(actual_game_export, expected_game_export);
    }

    // Game with regulation and shootout goals should only have the regulation
    // goals
    #[test]
    fn extract_export_game_data_regl_so() {
        let period_1 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![
                GoalInfo { eventId: 12, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("right"), isHome: false}
            ]
        };
        let period_2 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![]
        };
        let period_3 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![]
        };
        let ot = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("OT"),
            },
            goals: vec![]
        };
        let shootout = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("SO"),
            },
            goals: vec![
                GoalInfo { eventId: 486, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("right"), isHome: false}
            ]
        };
        let summary = Summary { 
            scoring: vec![
                period_1,
                period_2, 
                period_3,
                ot,
                shootout
            ]
        };
        let landing_resp = LandingResponse { 
            id: 2024000201, season: 20242025, gameDate: String::from("2024-10-29"), 
            homeTeam: Team { id: 10 }, awayTeam: Team { id: 19 }, summary: summary };
        
        let actual_game_export = extract_export_game_data(&landing_resp).unwrap();
        let expected_game_export = GameExportData {
            home_team_id: 10,
            goals: vec![
                GoalDetails {
                    event_id: 12,
                    ppt_replay_url: Some(String::from("nhl.com")),
                    scoring_team_id: 19,
                    home_team_defending_side: IceSide::Right
                },
            ]
        };

        assert_eq!(actual_game_export, expected_game_export);
    }

    // Game with regulation and an overtime goal should have all the goals
    #[test]
    fn extract_export_game_data_regl_ot() {
        let period_1 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![
                GoalInfo { eventId: 12, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("right"), isHome: false}
            ]
        };
        let period_2 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![
                GoalInfo { eventId: 200, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("left"), isHome: false}
            ]
        };
        let period_3 = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("REG"),
            },
            goals: vec![
                GoalInfo { eventId: 312, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("right"), isHome: true},
                GoalInfo { eventId: 351, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("right"), isHome: true}
            ]
        };
        let ot = Period { 
            periodDescriptor: PeriodDetails { 
                periodType: String::from("OT"),
            },
            goals: vec![
                GoalInfo { eventId: 1114, pptReplayUrl: Some(String::from("nhl.com")), homeTeamDefendingSide: String::from("left"), isHome: true}
            ]
        };
        let summary = Summary { 
            scoring: vec![
                period_1,
                period_2, 
                period_3,
                ot,
            ]
        };
        let landing_resp = LandingResponse { 
            id: 2024000201, season: 20242025, gameDate: String::from("2024-10-29"), 
            homeTeam: Team { id: 10 }, awayTeam: Team { id: 19 }, summary: summary };
        
        let actual_game_export = extract_export_game_data(&landing_resp).unwrap();
        let expected_game_export = GameExportData {
            home_team_id: 10,
            goals: vec![
                GoalDetails {
                    event_id: 12,
                    ppt_replay_url: Some(String::from("nhl.com")),
                    scoring_team_id: 19,
                    home_team_defending_side: IceSide::Right
                },
                GoalDetails {
                    event_id: 200,
                    ppt_replay_url: Some(String::from("nhl.com")),
                    scoring_team_id: 19,
                    home_team_defending_side: IceSide::Left
                },
                GoalDetails {
                    event_id: 312,
                    ppt_replay_url: Some(String::from("nhl.com")),
                    scoring_team_id: 10,
                    home_team_defending_side: IceSide::Right
                },
                GoalDetails {
                    event_id: 351,
                    ppt_replay_url: Some(String::from("nhl.com")),
                    scoring_team_id: 10,
                    home_team_defending_side: IceSide::Right
                },
                GoalDetails {
                    event_id: 1114,
                    ppt_replay_url: Some(String::from("nhl.com")),
                    scoring_team_id: 10,
                    home_team_defending_side: IceSide::Left
                },
            ]
        };

        assert_eq!(actual_game_export, expected_game_export);
    }
}
