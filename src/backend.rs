use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use chrono::SecondsFormat;
use mania_converter::osu_func::{OsuDataV128, calculate_from_data};
use osu_db::{Listing, Mod, ModSet, Mode, ScoreList};
use rayon::prelude::*;

use crate::Play;

const SCORE_V2: u32 = 1 << 29;

fn accuracies(j: [u32; 6], mods: ModSet) -> Option<(f64, f64)> {
    // Widen each replay counter before summing (the DB stores them as u16).
    let total = f64::from(j.iter().sum::<u32>());
    if total == 0.0 {
        return None;
    }
    let [max, n300, n200, n100, n50, _] = j.map(f64::from);
    let weighted = |max_weight: f64| {
        max_weight * max + 300.0 * n300 + 200.0 * n200 + 100.0 * n100 + 50.0 * n50
    };
    let acc = if mods.bits() & SCORE_V2 != 0 {
        weighted(305.0) / (3.05 * total)
    } else {
        weighted(300.0) / (3.0 * total)
    };
    Some((acc, weighted(310.0) / (3.1 * total)))
}

fn calc_rating(level: f64, acc: f64) -> f64 {
    let lower = (level - 3.0).max(0.0);
    if !(0.0..=100.0).contains(&acc) || acc <= 80.0 {
        0.0
    } else if acc <= 93.0 {
        lower * (acc - 80.0) / 13.0
    } else if acc <= 96.0 {
        (level - lower) * (acc - 93.0) / 3.0 + lower
    } else if acc <= 98.0 {
        let x = acc - 96.0;
        1.5 * x / (3.0 - x / 2.0) + level
    } else if acc <= 99.5 {
        let x = (acc - 98.0) / 1.5;
        2.0 * x * 2.0 / (3.0 - x) + level + 1.5
    } else {
        let x = (acc - 99.5) * 2.0;
        x * 2.0 / (3.0 - x) + level + 3.5
    }
}

#[derive(Clone, Copy)]
enum Variant {
    Normal,
    Hr,
    Ht,
    Dt,
}

fn variant(mods: ModSet) -> Variant {
    // As in the GUI, speed-mod SR uses original OD even when HR is set.
    if mods.contains(Mod::HalfTime) {
        Variant::Ht
    } else if mods.contains(Mod::DoubleTime) || mods.contains(Mod::Nightcore) {
        Variant::Dt
    } else if mods.contains(Mod::HardRock) {
        Variant::Hr
    } else {
        Variant::Normal
    }
}

struct Score {
    player: String,
    mods: ModSet,
    time: String,
    judgments: [u32; 6],
    acc: f64,
    rating_acc: f64,
}

fn rate_map(path: &Path, hash: &str, scores: Vec<Score>) -> Result<Vec<Play>> {
    let mut data =
        OsuDataV128::from_file(path.to_str().context("non-UTF-8 map path")?)?.to_legacy();
    ensure!(
        data.misc.circle_size == 6 && !data.notes.is_empty(),
        "not a nonempty 6K chart"
    );
    let info = data.to_beatmap_info(false);
    let artist = info
        .artist_unicode
        .filter(|s| !s.is_empty())
        .unwrap_or(info.artist);
    let title = info
        .title_unicode
        .filter(|s| !s.is_empty())
        .unwrap_or(info.title);
    let mut levels = [None; 4];
    let mut plays = Vec::with_capacity(scores.len());
    for score in scores {
        let v = variant(score.mods);
        let level = match levels[v as usize] {
            Some(value) => value,
            None => {
                let sr = match v {
                    Variant::Normal => calculate_from_data(&data, 1.0)?,
                    Variant::Ht => calculate_from_data(&data, 0.75)?,
                    Variant::Dt => calculate_from_data(&data, 1.5)?,
                    Variant::Hr => {
                        let od = data.misc.od;
                        let window = 64.5 - (od * 3.0).ceil();
                        data.misc.od = (64.5 - window / 1.4) / 3.0;
                        let sr = calculate_from_data(&data, 1.0)?;
                        data.misc.od = od;
                        sr
                    }
                };
                ensure!(sr.is_finite(), "non-finite star rating");
                let value = sr * 200.0 / 81.0 + 7.0 / 6.0;
                levels[v as usize] = Some(value);
                value
            }
        };
        plays.push(Play {
            hash: hash.to_owned(),
            artist: artist.clone(),
            title: title.clone(),
            creator: info.creator.clone(),
            version: info.version.clone(),
            player: score.player,
            time: score.time,
            mods: score.mods.bits(),
            notes: info.note_count,
            lns: info.ln_count,
            judgments: score.judgments,
            acc: score.acc,
            rating_acc: score.rating_acc,
            level,
            rating: calc_rating(level, score.rating_acc),
        });
    }
    Ok(plays)
}

pub fn extract(osu_dir: &Path, songs_dir: &Path) -> Result<Vec<Play>> {
    let db = osu_dir.join("osu!.db");
    let listing = Listing::from_file(&db).with_context(|| format!("read {}", db.display()))?;
    let mut maps: BTreeMap<String, (PathBuf, Vec<Score>)> = BTreeMap::new();
    for map in listing.beatmaps {
        if map.circle_size == 6.0 && map.mode == Mode::Mania {
            match (map.hash, map.folder_name, map.file_name) {
                (Some(hash), Some(folder), Some(file)) => {
                    maps.insert(hash, (songs_dir.join(folder).join(file), Vec::new()));
                }
                fields => eprintln!("Skipping 6K map with incomplete index entry: {fields:?}"),
            }
        }
    }

    let db = osu_dir.join("scores.db");
    let scores = ScoreList::from_file(&db).with_context(|| format!("read {}", db.display()))?;
    for replay in scores.beatmaps.into_iter().flat_map(|b| b.scores) {
        if replay.mode != Mode::Mania
            || replay.mods.contains(Mod::Random)
            || replay.mods.contains(Mod::Easy)
        {
            continue;
        }
        let (Some(hash), Some(player)) = (replay.beatmap_hash, replay.player_name) else {
            continue;
        };
        let Some((_, plays)) = maps.get_mut(&hash) else {
            continue;
        };
        let judgments = [
            replay.count_geki,
            replay.count_300,
            replay.count_katsu,
            replay.count_100,
            replay.count_50,
            replay.count_miss,
        ]
        .map(u32::from);
        if let Some((acc, rating_acc)) = accuracies(judgments, replay.mods) {
            plays.push(Score {
                player,
                mods: replay.mods,
                judgments,
                acc,
                rating_acc,
                time: replay.timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true),
            });
        }
    }

    let played: Vec<_> = maps
        .into_iter()
        .filter(|(_, (_, s))| !s.is_empty())
        .collect();
    let batches: Vec<_> = played
        .into_par_iter()
        .map(|(hash, (path, scores))| {
            // Some malformed charts can panic inside the pinned SR implementation.
            // Isolate the converter so one such chart does not discard other maps.
            match std::panic::catch_unwind(|| rate_map(&path, &hash, scores)) {
                Ok(Ok(plays)) => return plays,
                Ok(Err(e)) => eprintln!("Skipping map {hash} ({}): {e:#}", path.display()),
                Err(_) => eprintln!(
                    "Skipping map {hash} ({}): converter panicked",
                    path.display()
                ),
            }
            Vec::new()
        })
        .collect();
    Ok(batches.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judgment_weights_and_large_counts() {
        let j = [40_000, 40_000, 0, 0, 0, 0];
        let (v1, r1) = accuracies(j, ModSet(0)).unwrap();
        let (v2, r2) = accuracies(j, ModSet(SCORE_V2)).unwrap();
        assert_eq!(v1, 100.0);
        assert!((v2 - 60500.0 / 610.0).abs() < 1e-10);
        assert!((r1 - 61000.0 / 620.0).abs() < 1e-10);
        assert_eq!(r1, r2);
        assert_eq!(accuracies([0; 6], ModSet(0)), None);
    }

    #[test]
    fn rating_breakpoints() {
        for (acc, expected) in [
            (80.0, 0.0),
            (93.0, 7.0),
            (96.0, 10.0),
            (98.0, 11.5),
            (99.5, 13.5),
            (100.0, 14.5),
        ] {
            assert_eq!(calc_rating(10.0, acc), expected);
        }
        assert_eq!(calc_rating(2.0, 93.0), 0.0);
    }

    #[test]
    fn speed_mod_precedence() {
        assert!(matches!(variant(ModSet((1 << 4) | (1 << 9))), Variant::Dt));
        assert!(matches!(variant(ModSet((1 << 6) | (1 << 8))), Variant::Ht));
        assert!(matches!(variant(ModSet(1 << 4)), Variant::Hr));
    }
}
