use crate::Play;
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::Path,
};

const BEST_HEADER: &str = "hash,artist,title,creator,version,player,time,mods,notes,lns,max,300,200,100,50,miss,acc,rating_acc,level,rating,delta";

/// Export unfiltered and filtered map bests, and summarize the latter.
/// Rows for equal best ratings use the last occurrence in the input slice.
pub fn export(plays: &[Play], out: &Path) -> Result<()> {
    for (i, p) in plays.iter().enumerate() {
        ensure!(
            p.acc.is_finite() && p.rating_acc.is_finite() && p.rating.is_finite(),
            "non-finite score at play {}",
            i + 1
        );
        ensure!(
            (i64::MIN as f64..-(i64::MIN as f64)).contains(&p.level),
            "invalid level at play {}",
            i + 1
        );
    }
    fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    for n in 0u64.. {
        let suffix = if n == 0 {
            stamp.clone()
        } else {
            format!("{stamp}_{n}")
        };
        let best = out.join(format!("best_{suffix}.csv"));
        let stats = out.join(format!("stats_{suffix}.csv"));
        let best_stats = out.join(format!("best_stats_{suffix}.csv"));
        if stats.try_exists()? || best_stats.try_exists()? {
            continue;
        }
        let file = match File::create_new(&best) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("create {}", best.display())),
        };
        let rows = filtered_best(plays);
        write_file(file, &best, |w| write_best(w, &rows))?;
        let file =
            File::create_new(&stats).with_context(|| format!("create {}", stats.display()))?;
        write_file(file, &stats, |w| write_best(w, &map_bests(plays)))?;
        let file = File::create_new(&best_stats)
            .with_context(|| format!("create {}", best_stats.display()))?;
        write_file(file, &best_stats, |w| write_best_stats(w, &rows))?;
        println!(
            "{}\n{}\n{}\n筛选后谱面数：{}",
            stats.display(),
            best.display(),
            best_stats.display(),
            rows.len()
        );
        return Ok(());
    }
    anyhow::bail!("没有可用的输出文件名")
}

fn write_file(
    file: File,
    path: &Path,
    f: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> Result<()> {
    (|| {
        let mut w = BufWriter::new(file);
        f(&mut w)?;
        w.flush()
    })()
    .with_context(|| format!("write {}", path.display()))
}

fn text(w: &mut impl Write, s: &str) -> io::Result<()> {
    if s.contains([',', '"', '\n', '\r']) {
        write!(w, "\"{}\"", s.replace('"', "\"\""))
    } else {
        w.write_all(s.as_bytes())
    }
}

fn filtered_best(plays: &[Play]) -> Vec<&Play> {
    map_bests(plays.iter().filter(|p| {
        let total = u64::from(p.notes) + u64::from(p.lns);
        p.rating_acc >= 96.0 && total > 0 && 4 * u64::from(p.lns) <= total
    }))
}

fn map_bests<'a>(plays: impl IntoIterator<Item = &'a Play>) -> Vec<&'a Play> {
    let mut best = BTreeMap::new();
    for p in plays {
        let current = best.entry(p.hash.as_str()).or_insert(p);
        if p.rating >= current.rating {
            *current = p;
        }
    }
    // The stable sort preserves hash order when ratings compare equal.
    let mut rows: Vec<_> = best.into_values().collect();
    rows.sort_by(|a, b| b.rating.partial_cmp(&a.rating).expect("finite ratings"));
    rows
}

fn write_best(w: &mut impl Write, rows: &[&Play]) -> io::Result<()> {
    write!(w, "{BEST_HEADER}\r\n")?;
    for p in rows {
        for s in [
            &p.hash, &p.artist, &p.title, &p.creator, &p.version, &p.player, &p.time,
        ] {
            text(w, s)?;
            w.write_all(b",")?;
        }
        write!(w, "{},{},{}", p.mods, p.notes, p.lns)?;
        for n in p.judgments {
            write!(w, ",{n}")?;
        }
        write!(
            w,
            ",{:.8},{:.8},{:.8},{:.8},{:.8}\r\n",
            p.acc,
            p.rating_acc,
            p.level,
            p.rating,
            p.rating - p.level
        )?;
    }
    Ok(())
}

// P95 uses nearest rank; the top 5% includes exactly ceil(n/20) observations.
fn summary(v: &mut [f64]) -> [f64; 4] {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let top = &v[n - n.div_ceil(20)..];
    let mean = |s: &[f64]| s.iter().sum::<f64>() / s.len() as f64;
    [mean(v), v[n - 1 - n / 20], mean(top), v[n - 1]]
}

fn write_best_stats(w: &mut impl Write, rows: &[&Play]) -> io::Result<()> {
    w.write_all(b"level_floor,count,mean_delta,p95_delta,top5_mean_delta,best_delta\r\n")?;
    let mut groups = BTreeMap::<_, Vec<f64>>::new();
    for p in rows {
        groups
            .entry(p.level.floor() as i64)
            .or_default()
            .push(p.rating - p.level);
    }
    for (level, mut values) in groups {
        let count = values.len();
        let [mean, p95, top5, best] = summary(&mut values);
        write!(
            w,
            "{level},{count},{mean:.8},{p95:.8},{top5:.8},{best:.8}\r\n"
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(hash: &str, player: &str, rating: f64, level: f64) -> Play {
        Play {
            hash: hash.into(),
            player: player.into(),
            rating,
            level,
            artist: String::new(),
            title: String::new(),
            creator: String::new(),
            version: String::new(),
            time: String::new(),
            mods: 0,
            notes: 1,
            lns: 0,
            judgments: [0; 6],
            acc: 1.0,
            rating_acc: 1.0,
        }
    }

    #[test]
    fn best_is_global_last_tie_and_csv_safe() {
        let mut chosen = play("a", "last", 8.0, 1.0);
        chosen.title = "曲,\"名\"\r\n".into();
        let plays = [
            play("z", "z", 8.0, 1.0),
            play("a", "old", 8.0, 1.0),
            chosen,
            play("a", "worse", 7.0, 1.0),
            play("c", "first", 9.0, 1.0),
        ];
        let mut buf = Vec::new();
        let mut plays = plays;
        for p in &mut plays {
            p.rating_acc = 96.0;
        }
        write_best(&mut buf, &filtered_best(&plays)).unwrap();
        let csv = String::from_utf8(buf).unwrap();
        let rows = csv.strip_prefix(&format!("{BEST_HEADER}\r\n")).unwrap();
        assert!(rows.starts_with("c,"));
        assert!(
            rows.lines()
                .next()
                .unwrap()
                .ends_with(",1.00000000,9.00000000,8.00000000")
        );
        assert!(
            rows.find("\r\na,,\"曲,\"\"名\"\"\r\n\",,,last,").unwrap()
                < rows.find("\r\nz,").unwrap()
        );
        assert!(!rows.contains("old") && !rows.contains("worse"));
    }

    #[test]
    fn unfiltered_stats_keep_low_acc_and_ln_heavy_winners() {
        let plays = [
            play("a", "one", 5.0, 7.0),
            Play {
                notes: 0,
                lns: 9,
                ..play("a", "two", 6.0, 7.0)
            },
            play("b", "one", 4.0, 7.0),
        ];
        let rows = map_bests(&plays);
        assert_eq!(
            rows.iter()
                .map(|p| (p.hash.as_str(), p.player.as_str()))
                .collect::<Vec<_>>(),
            [("a", "two"), ("b", "one")]
        );
        assert!(filtered_best(&plays).is_empty());
    }

    #[test]
    fn filters_before_selecting_best_and_binning() {
        let base = Play {
            notes: 3,
            lns: 1,
            rating_acc: 96.0,
            ..play("a", "first", 11.0, 10.5)
        };
        let plays = [
            Play {
                rating: 20.0,
                rating_acc: 95.999,
                ..base.clone()
            },
            base.clone(),
            Play {
                hash: "b".into(),
                lns: 2,
                ..base.clone()
            },
            Play {
                hash: "c".into(),
                notes: 0,
                lns: 0,
                ..base.clone()
            },
            Play {
                player: "other".into(),
                rating: 14.0,
                level: 11.0,
                ..base
            },
        ];
        let rows = filtered_best(&plays);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].player, "other");
        let mut buf = Vec::new();
        write_best_stats(&mut buf, &rows).unwrap();
        assert_eq!(
            String::from_utf8(buf)
                .unwrap()
                .lines()
                .skip(1)
                .collect::<Vec<_>>(),
            ["11,1,3.00000000,3.00000000,3.00000000,3.00000000"]
        );
    }

    #[test]
    fn nearest_rank_and_top_five_percent() {
        let mut values: Vec<f64> = (1..=21).rev().map(f64::from).collect();
        assert_eq!(summary(&mut values), [11.0, 20.0, 20.5, 21.0]);
    }

    #[test]
    fn empty_reports_still_have_headers() {
        let (mut best, mut stats) = (Vec::new(), Vec::new());
        write_best(&mut best, &[]).unwrap();
        write_best_stats(&mut stats, &[]).unwrap();
        assert_eq!(best, format!("{BEST_HEADER}\r\n").as_bytes());
        assert_eq!(
            stats,
            b"level_floor,count,mean_delta,p95_delta,top5_mean_delta,best_delta\r\n"
        );
    }
}
