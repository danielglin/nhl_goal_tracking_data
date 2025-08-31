use chrono::NaiveDate;

use reqwest;
use reqwest::blocking::Client;
use reqwest::header::HeaderMap;

use serde::{Deserialize, Serialize};
// use serde_json::{Value};

use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::api_calls::week_or_shorter_period::WeekOrShorterPeriod;

// pub struct Season(pub String);
// #[derive(Clone, Debug)]
// pub struct GameId(pub String);
// pub struct GoalId(pub u16);

/// Saves the tracking data for a goal to a file
/// This requires headers to get the data from the NHL site.
pub fn save_goal_data<P>(
    client: &Client,
    headers: HeaderMap,
    game: &Game,
    goal: &GoalDetails,
    output_path: P,
) -> Result<()>
where
    P: AsRef<Path>,
{
    // get the tracking data
    let api_url = match &goal.ppt_replay_url {
        Some(url) => url.to_string(),
        None => format!(
            "https://wsr.nhle.com/sprites/{}/{}/ev{}.json",
            game.season, game.id, goal.event_id
        ),
    };
    let resp = client.get(api_url).headers(headers).send()?;
    if resp.status() == 200 {
        let resp_text = resp.text()?;

        // save the data to a file
        let mut file = File::create(output_path).with_context(|| {
            format!(
                "Failed to write tracking data for season: {}, game id: {}, goal id: {} to a file",
                game.season, game.id, goal.event_id
            )
        })?;
        write!(file, "{}", resp_text)?;
        Ok(())
    } else {
        let err_msg = format!(
            "Unable to get data for season: {}, game id: {}, goal id: {}",
            game.season, game.id, goal.event_id
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
    //     schedule: &ScheduleResponse,
    // start_date: &NaiveDate,
    // end_date: &NaiveDate
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
}

/// play-by-play info along with the game id
#[derive(Debug)]
pub struct PbpInfo {
    plays: Vec<Event>,
    game_id: u32,
}

#[derive(Deserialize, Debug)]
pub struct Event {
    eventId: u32,
    homeTeamDefendingSide: String,
    // typeCode: u16,
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
    // event_id: u32,
    // scoring_team_id: u16,
    // home_team_defending_side: String,
    goals: Vec<GoalDetails>,
    home_team_id: u16,
}

/// Get the pbp data for a game
pub fn get_pbp_data(client: &Client, game: &Game) -> Result<PbpInfo> {
    let pbp_url = format!(
        "https://api-web.nhle.com/v1/gamecenter/{}/play-by-play",
        game.id
    );
    let resp = client.get(pbp_url).send()?;

    if resp.status() == 200 {
        let resp_text = resp.text()?;
        let pbp_resp: PbpResponse = serde_json::from_str(&resp_text)?;

        // add in the game id so we don't have just play-by-play
        // info withou a way to tie back to a game
        let pbp_info = PbpInfo {
            plays: pbp_resp.plays,
            game_id: game.id,
        };
        Ok(pbp_info)
    } else {
        let err_msg = format!("Unable to get play-by-play for game id: {}, response status: {}.", game.id, resp.status());
        Err(anyhow!(err_msg))
    }
}

/// From data returned by the play-by-play API, get just the goal
/// data for a game
pub fn parse_goal_data(mut pbp: PbpInfo) -> Vec<GoalDetails> {
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
            println!("Invalid side for goal {} in game {}", event_id, pbp.game_id);
            continue;
        };

        // // get the location JSON URL
        // let ppt_replay_url = match goal_event.pptReplayUrl {
        //     Some(url) => url,
        //     None => {
        //         println!("No location JSON URL found for goal {} in game {}", event_id, pbp.game_id);
        //         continue;
        //     }
        // };

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
                            event_id, pbp.game_id
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
                println!("No details for goal {} in game {}", event_id, pbp.game_id);
                continue;
            }
        }
    }
    goals
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

#[derive(Deserialize, Debug)]
pub struct Team {
    id: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct BoxscoreInfo {
    // game_id: u32,
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
        let pbp_info = PbpInfo { game_id: 1, plays };

        let actual_goal_details = parse_goal_data(pbp_info);

        assert_eq!(actual_goal_details.len(), 0);
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
        let pbp_info = PbpInfo { game_id: 1, plays };

        let actual_goal_details = parse_goal_data(pbp_info);
        let expected_goal_details = vec![GoalDetails {
            event_id: 90,
            home_team_defending_side: IceSide::Right,
            ppt_replay_url: Some(String::from("nhl.com")),
            scoring_team_id: 1,
        }];

        assert_eq!(actual_goal_details, expected_goal_details);
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
        let pbp_info = PbpInfo { game_id: 1, plays };

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

        assert_eq!(actual_goal_details, expected_goal_details);
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
        let pbp_info = PbpInfo { game_id: 2, plays };

        let actual_goal_details = parse_goal_data(pbp_info);
        let expected_goal_details = vec![];

        assert_eq!(actual_goal_details, expected_goal_details);
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
        let pbp_info = PbpInfo { game_id: 2, plays };

        let actual_goal_details = parse_goal_data(pbp_info);
        let expected_goal_details = vec![GoalDetails {
            event_id: 93,
            home_team_defending_side: IceSide::Right,
            ppt_replay_url: Some(String::from("nhl.com/ev93")),
            scoring_team_id: 1,
        }];

        assert_eq!(actual_goal_details, expected_goal_details);
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
        let pbp_info = PbpInfo { game_id: 2, plays };

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

        assert_eq!(actual_goal_details, expected_goal_details);
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
        let pbp_info = PbpInfo { game_id: 2, plays };

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

        assert_eq!(actual_goal_details, expected_goal_details);
    }
}
