//! The data a map view needs: regions with circles and labels, and one point per item.

use anyhow::{Context, Result};
use invectus_core::Project;
use invectus_core::store::{RegionRecord, RegionRun, VectorTableRecord};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct MapPoint {
    pub item_id: i64,
    pub title: Option<String>,
    pub role: String,
    pub date: Option<String>,
    pub url: Option<String>,
    pub x: f32,
    pub y: f32,
    pub fine_id: i32,
    pub broad_id: i32,
    pub noise: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct MapData {
    pub run: RegionRun,
    pub regions: Vec<RegionRecord>,
    pub points: Vec<MapPoint>,
    /// Always shown with the map: positions are for browsing, not evidence.
    pub note: String,
}

/// Map data from the latest region run of a collection's vector space.
pub fn map_data(project: &Project, table: &VectorTableRecord) -> Result<MapData> {
    let run = project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?
        .with_context(|| {
            format!(
                "collection {:?} has no regions for space {}; run `invectus regions run -c {}`",
                table.collection_id,
                &table.space_id[..12],
                table.collection_id
            )
        })?;
    let regions = project.store.regions(&run.run_id)?;
    let members = project.store.region_members(&run.run_id)?;
    let ids: Vec<i64> = members.iter().map(|m| m.item_id).collect();
    let items = project.store.items_by_ids(&ids)?;
    let points = members
        .iter()
        .map(|m| {
            let item = items.get(&m.item_id);
            MapPoint {
                item_id: m.item_id,
                title: item.and_then(|i| i.title.clone()),
                role: item.map_or("neutral", |i| i.role.as_str()).into(),
                date: item.and_then(|i| i.date.clone()),
                url: item.and_then(|i| i.url.clone()),
                x: m.x,
                y: m.y,
                fine_id: m.fine_id,
                broad_id: m.broad_id,
                noise: m.noise,
            }
        })
        .collect();
    Ok(MapData {
        run,
        regions,
        points,
        note: "map positions are for browsing only; distances on the map are approximate and are \
               never used to decide regions or gaps"
            .into(),
    })
}
