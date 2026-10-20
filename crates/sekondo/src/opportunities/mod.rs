//! Station trading opportunities: analyses of the built-in trading station's market.

pub mod analysis;
pub mod charts;
pub mod legend;
pub mod page;
pub mod settings;
pub mod stats;
pub mod tables;

/// The one trading station: Jita IV - Moon 4 - Caldari Navy Assembly Plant, in The Forge. All
/// station data is stored by region and station, but there is no way to choose another.
pub const REGION_ID: i64 = 10_000_002;
pub const STATION_ID: i64 = 60_003_760;
pub const STATION_NAME: &str = "Jita IV - Moon 4 - Caldari Navy Assembly Plant";
