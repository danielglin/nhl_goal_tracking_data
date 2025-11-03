# Introduction

This is a command line tool written in Rust that downloads puck and player tracking data for NHL goals.
This tracking data is the same data used to create the goal visualizations on nhl.com.
See one example [here](https://www.nhl.com/ppt-replay/goal/2024020999/663).

# Usage

## Options

Users can choose to download the goal tracking data for all goals in a date range, or for all goals in a specific game:
- `--dates`: a date range in "YYYY-MM-DD::YYYY-MM-DD" format
    - Example: "2024-11-01::2024-11-03" downloads all goal tracking info from November 1, 2024 to November 3, 2024. Each date gets its own folder, and within each date folder are subfolders for each game.
- `--game`: a game id
    - Game id's can be found in the URL of a game's Gamecenter page.  For example, the October 26, 2025 game between the Devils and Avalanche has its Gamecenter page at https://www.nhl.com/gamecenter/col-vs-njd/2025/10/26/2025020140, and the game id is the last part, `2025020140`.


## Examples Using Cargo

Using Cargo, you can pull the data for a game like:

```
$ cargo run --release -- --game 2025020140 --output "example_output/"
```
This saves the goals for game 2025020140 to the `example_output/` folder.  A folder for the date of the game, `2025-10-26`, is created, as is a folder for the game `2025020140`.  In that game folder, there is one JSON file per non-shootout goal, as well as a `pbp_boxscore.json` file that has information about which team scored, the event id, and details to determine which side of the ice the goal was scored on.


Example of pulling all data within a date range:

```
$ cargo run --release -- --dates 2025-10-29::2025-10-31 --output "example_output/"
```
This saves the goals for all games from October 29, 2025 to October 31, 2025 to the `example_output/` folder.  A folder is created for each date, and within each date's folder are separate folders for each game.  Just like pulling data for a single game, there is one JSON file for each non-shootout goal plus a `pbp_boxscore.json` file with additional information.

# Ackknowledgements

Stick tap to [Zmalski's NHL API Documentation repo](https://github.com/Zmalski/NHL-API-Reference).