//! End-to-end tests of the video tools against a real FFmpeg.
//!
//! Inputs are made once per run in `target/tmp` with FFmpeg's test sources: a 720p MKV with
//! sound and a subtitle track, a silent MP4, a one-minute clip for cancelling and a file that
//! only pretends to be a video. Every test skips itself when ffmpeg or ffprobe is missing.

use converty_core::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn has_ffmpeg() -> bool {
    let d = deps();
    let ok = d.has("ffmpeg") && d.has("ffprobe");
    if !ok {
        eprintln!("ffmpeg/ffprobe not installed, skipping");
    }
    ok
}

fn encoders() -> &'static HashSet<String> {
    static ENC: OnceLock<HashSet<String>> = OnceLock::new();
    ENC.get_or_init(|| {
        let out = Command::new("ffmpeg").args(["-hide_banner", "-encoders"]).output().expect("run ffmpeg");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
            .collect()
    })
}

fn has_encoder(name: &str) -> bool {
    encoders().contains(name)
}

fn ffmpeg(args: &[&str]) {
    let out = Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).output().expect("run ffmpeg");
    assert!(out.status.success(), "ffmpeg {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

struct Fx {
    /// 1280x720, 2 s, H.264 (or MPEG-4 Part 2) + AAC + SRT subtitles
    clip: PathBuf,
    /// 320x240, 1 s, no audio
    silent: PathBuf,
    /// 640x360, 60 s
    long: PathBuf,
    broken: PathBuf,
}

fn fx() -> &'static Fx {
    static FX: OnceLock<Fx> = OnceLock::new();
    FX.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("video-fixtures");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let srt = dir.join("subs.srt");
        std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:01,500\nHello\n").unwrap();
        let vcodec = if has_encoder("libx264") { "libx264" } else { "mpeg4" };
        let p = |n: &str| dir.join(n).to_string_lossy().into_owned();

        let clip = p("clip.mkv");
        ffmpeg(&[
            "-f", "lavfi", "-i", "testsrc=duration=2:size=1280x720:rate=25",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=2:sample_rate=48000",
            "-i", &srt.to_string_lossy(),
            "-map", "0", "-map", "1", "-map", "2", "-c:v", vcodec, "-pix_fmt", "yuv420p", "-c:a", "aac", "-c:s", "srt", &clip,
        ]);
        let silent = p("silent.mp4");
        ffmpeg(&["-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25", "-c:v", "mpeg4", &silent]);
        let long = p("long.mkv");
        ffmpeg(&[
            "-f", "lavfi", "-i", "testsrc=duration=60:size=640x360:rate=25",
            "-f", "lavfi", "-i", "sine=duration=60", "-c:v", "mpeg4", "-q:v", "10", "-c:a", "aac", &long,
        ]);
        let broken = dir.join("broken.mp4");
        std::fs::write(&broken, b"this is not a video at all".repeat(100)).unwrap();
        Fx { clip: clip.into(), silent: silent.into(), long: long.into(), broken }
    })
}

/// (codec_type, codec_name, height) of every stream.
fn streams(p: &Path) -> Vec<(String, String, u32)> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type,codec_name,height", "-of", "csv=p=0"])
        .arg(p)
        .output()
        .expect("run ffprobe");
    assert!(out.status.success(), "{} is not readable: {}", p.display(), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            (f.get(1).unwrap_or(&"").to_string(), f[0].to_string(), f.get(2).and_then(|h| h.parse().ok()).unwrap_or(0))
        })
        .collect()
}

fn kinds(p: &Path) -> Vec<String> {
    streams(p).into_iter().map(|s| s.0).collect()
}

fn codec_of(p: &Path, kind: &str) -> String {
    streams(p).into_iter().find(|s| s.0 == kind).map(|s| s.1).unwrap_or_default()
}

fn job(tool: Tool, inputs: &[&Path], options: ToolOptions, dir: &Path, stem: &str) -> Result<JobResult> {
    run(
        &JobRequest {
            tool,
            inputs: inputs.iter().map(|p| Input { path: p.to_path_buf(), password: None }).collect(),
            options,
            output_dir: dir.to_path_buf(),
            output_stem: stem.into(),
        },
        &|_, _| {},
        &AtomicBool::new(false),
    )
}

fn convert(input: &Path, o: ConvertVideoOptions, dir: &Path) -> Result<JobResult> {
    job(Tool::ConvertVideo, &[input], ToolOptions::ConvertVideo(o), dir, "out")
}

fn ls(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn mkv_to_mp4_copies_streams_and_keeps_subtitles() {
    if !has_ffmpeg() {
        return;
    }
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let r = convert(&f.clip, ConvertVideoOptions::default(), dir.path()).unwrap();
    assert_eq!(ls(dir.path()), ["out.mp4"]);
    assert_eq!(r.outputs, [dir.path().join("out.mp4")]);
    assert_eq!(codec_of(&r.outputs[0], "video"), codec_of(&f.clip, "video"), "picture is copied");
    assert_eq!(codec_of(&r.outputs[0], "audio"), "aac");
    assert_eq!(codec_of(&r.outputs[0], "subtitle"), "mov_text");
    assert!(r.notes.iter().any(|n| n.contains("copied unchanged")), "{:?}", r.notes);
    assert!(r.output_bytes > 0 && r.input_bytes > 0);
}

#[test]
fn every_target_format_plays_back() {
    if !has_ffmpeg() {
        return;
    }
    let f = fx();
    for format in VideoFormat::ALL {
        if format == VideoFormat::Webm && !(has_encoder("libvpx-vp9") || has_encoder("libvpx")) {
            continue;
        }
        let dir = tempfile::tempdir().unwrap();
        let r = convert(&f.clip, ConvertVideoOptions { format, ..Default::default() }, dir.path())
            .unwrap_or_else(|e| panic!("{format:?}: {e}"));
        let out = &r.outputs[0];
        assert_eq!(out.extension().unwrap(), format.ext());
        let k = kinds(out);
        assert!(k.contains(&"video".to_string()) && k.contains(&"audio".to_string()), "{format:?}: {k:?}");
        match format {
            VideoFormat::Avi => {
                assert!(!k.contains(&"subtitle".to_string()), "{k:?}");
                assert_eq!(codec_of(out, "video"), "mpeg4");
                assert!(r.notes.iter().any(|n| n.contains("subtitle track left out")), "{:?}", r.notes);
            }
            VideoFormat::Webm => {
                assert!(["vp9", "vp8"].contains(&codec_of(out, "video").as_str()));
                assert!(["opus", "vorbis"].contains(&codec_of(out, "audio").as_str()));
                assert_eq!(codec_of(out, "subtitle"), "webvtt");
            }
            _ => assert!(k.contains(&"subtitle".to_string()), "{format:?}: {k:?}"),
        }
    }
}

#[test]
fn scaling_quality_and_removing_sound() {
    if !has_ffmpeg() {
        return;
    }
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let o = ConvertVideoOptions { resolution: VideoResolution::P480, quality: VideoQuality::Small, remove_audio: true, ..Default::default() };
    let r = convert(&f.clip, o, dir.path()).unwrap();
    let s = streams(&r.outputs[0]);
    assert_eq!(s.iter().find(|s| s.0 == "video").unwrap().2, 480);
    assert!(!s.iter().any(|s| s.0 == "audio"), "{s:?}");
    assert!(r.output_bytes < r.input_bytes, "Small at 480p should be smaller than the 720p input");

    // never upscaled: the silent clip is only 240 lines high
    let o = ConvertVideoOptions { resolution: VideoResolution::P720, format: VideoFormat::Mkv, ..Default::default() };
    let r = convert(&f.silent, o, dir.path()).unwrap();
    assert_eq!(streams(&r.outputs[0]).iter().find(|s| s.0 == "video").unwrap().2, 240);
}

#[test]
fn several_inputs_get_numbered_and_collisions_are_avoided() {
    if !has_ffmpeg() {
        return;
    }
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("out_1.mkv"), b"keep me").unwrap();
    let o = ConvertVideoOptions { format: VideoFormat::Mkv, ..Default::default() };
    let r = job(Tool::ConvertVideo, &[&f.clip, &f.silent], ToolOptions::ConvertVideo(o), dir.path(), "out").unwrap();
    assert_eq!(ls(dir.path()), ["out_1 (2).mkv", "out_1.mkv", "out_2.mkv"]);
    assert_eq!(r.outputs.len(), 2);
    assert_eq!(std::fs::read(dir.path().join("out_1.mkv")).unwrap(), b"keep me");
}

#[cfg(unix)]
#[test]
fn awkward_file_names_are_not_read_as_options_or_protocols() {
    if !has_ffmpeg() {
        return;
    }
    let src = tempfile::tempdir().unwrap();
    let odd = src.path().join("-i http: clip.mkv");
    std::fs::copy(&fx().silent, &odd).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let r = job(Tool::ConvertVideo, &[&odd], ToolOptions::ConvertVideo(ConvertVideoOptions::default()), dir.path(), "-y: odd").unwrap();
    assert_eq!(ls(dir.path()), ["-y: odd.mp4"]);
    assert_eq!(kinds(&r.outputs[0]), ["video"]);
}

#[test]
fn video_to_audio_all_formats() {
    if !has_ffmpeg() {
        return;
    }
    let f = fx();
    for format in AudioFormat::ALL {
        if (format == AudioFormat::Mp3 && !has_encoder("libmp3lame")) || (format == AudioFormat::Opus && !has_encoder("libopus")) {
            continue;
        }
        let dir = tempfile::tempdir().unwrap();
        let r = job(Tool::VideoToAudio, &[&f.clip], ToolOptions::VideoToAudio { format }, dir.path(), "sound")
            .unwrap_or_else(|e| panic!("{format:?}: {e}"));
        assert_eq!(ls(dir.path()), [format!("sound.{}", format.ext())]);
        assert_eq!(kinds(&r.outputs[0]), ["audio"], "{format:?}");
        let expected = match format {
            AudioFormat::Mp3 => "mp3",
            AudioFormat::M4a => "aac",
            AudioFormat::Wav => "pcm_s16le",
            AudioFormat::Flac => "flac",
            AudioFormat::Opus => "opus",
        };
        assert_eq!(codec_of(&r.outputs[0], "audio"), expected);
        assert_eq!(r.notes.iter().any(|n| n.contains("copied unchanged")), format == AudioFormat::M4a, "{:?}", r.notes);
    }
}

#[test]
fn missing_sound_broken_files_and_wrong_types_leave_nothing() {
    if !has_ffmpeg() {
        return;
    }
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let e = job(Tool::VideoToAudio, &[&f.silent], ToolOptions::VideoToAudio { format: AudioFormat::Wav }, dir.path(), "x").unwrap_err();
    assert!(e.to_string().contains("silent has no sound"), "{e}");

    let e = convert(&f.broken, ConvertVideoOptions::default(), dir.path()).unwrap_err();
    assert!(matches!(e, Error::External { program: "ffprobe", .. }), "{e:?}");

    // the second file fails: the first result is removed again
    let o = ToolOptions::VideoToAudio { format: AudioFormat::Wav };
    assert!(job(Tool::VideoToAudio, &[&f.clip, &f.silent], o, dir.path(), "x").is_err());

    let pdf = dir.path().join("doc.pdf");
    std::fs::write(&pdf, b"%PDF-1.4").unwrap();
    let e = convert(&pdf, ConvertVideoOptions::default(), dir.path()).unwrap_err();
    assert!(matches!(e, Error::WrongType(_)), "{e:?}");
    std::fs::remove_file(&pdf).unwrap();
    assert_eq!(ls(dir.path()), Vec::<String>::new());
}

#[test]
fn cancel_stops_ffmpeg_and_reports_progress() {
    if !has_ffmpeg() {
        return;
    }
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let max_seen = std::sync::Mutex::new(0f32);
    let req = JobRequest {
        tool: Tool::ConvertVideo,
        inputs: vec![Input { path: f.long.clone(), password: None }],
        // WebM/VP9 or a High re-encode keeps ffmpeg busy for a while
        options: ToolOptions::ConvertVideo(ConvertVideoOptions { quality: VideoQuality::High, ..Default::default() }),
        output_dir: dir.path().to_path_buf(),
        output_stem: "long".into(),
    };
    let start = Instant::now();
    let res = run(
        &req,
        &|frac, msg| {
            if msg.starts_with("Converting") && frac > 0.0 {
                *max_seen.lock().unwrap() = frac;
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    );
    assert!(matches!(res, Err(Error::Cancelled)), "{res:?}");
    let seen = *max_seen.lock().unwrap();
    assert!(seen > 0.0 && seen < 1.0, "progress {seen}");
    assert!(start.elapsed() < Duration::from_secs(30));
    assert_eq!(ls(dir.path()), Vec::<String>::new(), "no partial file left behind");
}
