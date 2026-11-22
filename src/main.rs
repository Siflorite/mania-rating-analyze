mod backend;
mod report;

use anyhow::{Context, Result, bail};
use std::{env, ffi::OsString, path::PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Play {
    pub hash: String,
    pub artist: String,
    pub title: String,
    pub creator: String,
    pub version: String,
    pub player: String,
    pub time: String,
    pub mods: u32,
    pub notes: u32,
    pub lns: u32,
    pub judgments: [u32; 6], // MAX, 300, 200, 100, 50, MISS
    pub acc: f64,
    pub rating_acc: f64,
    pub level: f64,
    pub rating: f64,
}

const HELP: &str = "用法：mania-rating-analyze [--osu-dir <osu! stable 安装目录>] [--songs-dir <谱面目录>] [--out <输出目录>]\n\n默认从 Windows 注册表定位 osu!，使用安装目录下的 Songs。输出到本程序旁的 rating_analyze_output，文件名带本地生成时间。";

#[cfg(windows)]
fn detect_osu_dir() -> Option<PathBuf> {
    use winreg::{
        RegKey,
        enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER},
    };
    [
        (HKEY_CLASSES_ROOT, r"osustable.File.osz\Shell\Open\Command"),
        (
            HKEY_CURRENT_USER,
            r"Software\Classes\osustable.File.osz\Shell\Open\Command",
        ),
    ]
    .into_iter()
    .find_map(|(root, key)| {
        let command: String = RegKey::predef(root)
            .open_subkey(key)
            .ok()?
            .get_value("")
            .ok()?;
        let command = command.trim_start();
        let exe = if let Some(quoted) = command.strip_prefix('"') {
            quoted.split_once('"')?.0
        } else {
            command.split_whitespace().next()?
        };
        let dir = std::path::Path::new(exe).parent()?;
        dir.join("osu!.db").is_file().then(|| dir.to_path_buf())
    })
}

#[cfg(not(windows))]
fn detect_osu_dir() -> Option<PathBuf> {
    None
}

struct Options {
    osu: PathBuf,
    songs: PathBuf,
    out: PathBuf,
}

fn options(mut args: impl Iterator<Item = OsString>) -> Result<Option<Options>> {
    let (mut osu, mut songs, mut out) = (None, None, None);
    while let Some(arg) = args.next() {
        let dest = match arg.to_str() {
            Some("-h" | "--help") => return Ok(None),
            Some("--osu-dir") => &mut osu,
            Some("--songs-dir") => &mut songs,
            Some("--out") => &mut out,
            _ => bail!("未知参数：{}\n{HELP}", arg.to_string_lossy()),
        };
        let value = args
            .next()
            .filter(|v| !v.is_empty() && !v.to_string_lossy().starts_with("--"))
            .with_context(|| format!("{} 需要目录路径", arg.to_string_lossy()))?;
        if dest.replace(PathBuf::from(value)).is_some() {
            bail!("重复参数：{}", arg.to_string_lossy());
        }
    }
    let osu = osu
        .or_else(detect_osu_dir)
        .context("无法自动定位 osu!.db，请用 --osu-dir 指定 osu! stable 安装目录")?;
    let out = match out {
        Some(out) => out,
        None => env::current_exe()?
            .parent()
            .context("无法定位本程序目录，请使用 --out")?
            .join("rating_analyze_output"),
    };
    Ok(Some(Options {
        songs: songs.unwrap_or_else(|| osu.join("Songs")),
        osu,
        out,
    }))
}

fn main() -> Result<()> {
    let Some(opts) = options(env::args_os().skip(1))? else {
        println!("{HELP}");
        return Ok(());
    };
    if !opts.songs.is_dir() {
        bail!(
            "谱面目录不存在：{}（可用 --songs-dir 指定）",
            opts.songs.display()
        );
    }
    let plays = backend::extract(&opts.osu, &opts.songs)?;
    report::export(&plays, &opts.out)?;
    println!(
        "有效游玩记录：{}；输出目录：{}",
        plays.len(),
        opts.out.display()
    );
    Ok(())
}
