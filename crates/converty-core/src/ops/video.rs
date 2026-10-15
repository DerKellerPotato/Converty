//! Video conversion and audio extraction through FFmpeg.
//!
//! Streams that already fit the target container are copied unchanged (fast and
//! lossless, e.g. the usual H.264 + AAC MKV to MP4); everything else is re-encoded
//! with the best encoder this FFmpeg build has.

use super::util::{file_size, os, run_cmd, run_cmd_lines};
use super::Ctx;
use crate::{AudioFormat, ConvertVideoOptions, Error, Result, VideoFormat, VideoQuality};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Video,
    Audio,
    Subtitle,
    Other,
}

#[derive(Debug, Clone)]
struct Stream {
    index: u32,
    kind: Kind,
    codec: String,
    height: u32,
    channels: u32,
    /// Cover art stored as a video stream.
    cover: bool,
}

#[derive(Debug, Clone, Default)]
struct Probe {
    /// Seconds, when the container knows it.
    duration: Option<f64>,
    streams: Vec<Stream>,
}

impl Probe {
    fn of(&self, kind: Kind) -> impl Iterator<Item = &Stream> {
        self.streams.iter().filter(move |s| s.kind == kind && !s.cover)
    }
}

/// What to pass to ffmpeg between the input and the output file.
#[derive(Debug, Default)]
struct Plan {
    args: Vec<String>,
    notes: Vec<String>,
}

/// `file:` keeps ffmpeg from reading a name like `a:b.mkv` as a protocol or `-x.mkv` as an option.
fn file_arg(p: &Path) -> OsString {
    let mut s = OsString::from("file:");
    s.push(p.as_os_str());
    s
}

fn probe(path: &Path, name: &str, cancel: &std::sync::atomic::AtomicBool) -> Result<Probe> {
    let args = [
        os("-v"),
        os("error"),
        os("-show_entries"),
        os("format=duration:stream=index,codec_type,codec_name,height,channels:stream_disposition=attached_pic"),
        os("-of"),
        os("json"),
        file_arg(path),
    ];
    let out = run_cmd("ffprobe", &args, cancel)?;
    if !out.success {
        return Err(Error::External { program: "ffprobe", message: format!("{name} could not be read as a video: {}", out.message()) });
    }
    parse_probe(&out.stdout).ok_or_else(|| Error::External { program: "ffprobe", message: format!("{name} could not be read as a video") })
}

fn parse_probe(json: &str) -> Option<Probe> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let num = |x: &serde_json::Value| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())).unwrap_or(0) as u32;
    let duration = v["format"]["duration"].as_str().and_then(|d| d.parse::<f64>().ok()).filter(|d| *d > 0.0);
    let streams = v["streams"]
        .as_array()?
        .iter()
        .map(|s| Stream {
            index: num(&s["index"]),
            kind: match s["codec_type"].as_str() {
                Some("video") => Kind::Video,
                Some("audio") => Kind::Audio,
                Some("subtitle") => Kind::Subtitle,
                _ => Kind::Other,
            },
            codec: s["codec_name"].as_str().unwrap_or_default().to_string(),
            height: num(&s["height"]),
            channels: num(&s["channels"]),
            cover: num(&s["disposition"]["attached_pic"]) == 1,
        })
        .collect();
    Some(Probe { duration, streams })
}

/// Names of the encoders this FFmpeg build has.
fn encoders(cancel: &std::sync::atomic::AtomicBool) -> Result<HashSet<String>> {
    let out = run_cmd("ffmpeg", &[os("-hide_banner"), os("-encoders")], cancel)?;
    if !out.success {
        return Err(Error::External { program: "ffmpeg", message: out.message() });
    }
    Ok(parse_encoders(&out.stdout))
}

/// Lines look like ` V....D libx264   libx264 H.264 / AVC ...`; the legend uses `=` as name.
fn parse_encoders(text: &str) -> HashSet<String> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let flags = it.next()?;
            let name = it.next()?;
            (flags.len() == 6 && flags.starts_with(['V', 'A', 'S']) && name != "=").then(|| name.to_string())
        })
        .collect()
}

fn video_fits(fmt: VideoFormat, codec: &str) -> bool {
    match fmt {
        VideoFormat::Mkv => !codec.is_empty(),
        VideoFormat::Mp4 => matches!(codec, "h264" | "hevc" | "mpeg4" | "av1" | "vp9"),
        VideoFormat::Mov => matches!(codec, "h264" | "hevc" | "mpeg4" | "prores" | "mjpeg"),
        VideoFormat::Webm => matches!(codec, "vp8" | "vp9" | "av1"),
        VideoFormat::Avi => matches!(codec, "mpeg4" | "mjpeg" | "msmpeg4v3"),
    }
}

fn audio_fits(fmt: VideoFormat, codec: &str) -> bool {
    match fmt {
        VideoFormat::Mkv => !codec.is_empty(),
        VideoFormat::Mp4 => matches!(codec, "aac" | "mp3" | "ac3" | "eac3" | "alac"),
        VideoFormat::Mov => matches!(codec, "aac" | "mp3" | "ac3" | "alac" | "pcm_s16le" | "pcm_s24le"),
        VideoFormat::Webm => matches!(codec, "vorbis" | "opus"),
        VideoFormat::Avi => matches!(codec, "mp3" | "ac3" | "mp2" | "pcm_s16le"),
    }
}

fn is_text_subtitle(codec: &str) -> bool {
    matches!(codec, "subrip" | "srt" | "ass" | "ssa" | "webvtt" | "mov_text" | "text")
}

fn format_label(fmt: VideoFormat) -> &'static str {
    match fmt {
        VideoFormat::Mp4 => "MP4",
        VideoFormat::Mkv => "MKV",
        VideoFormat::Webm => "WebM",
        VideoFormat::Mov => "MOV",
        VideoFormat::Avi => "AVI",
    }
}

fn muxer(fmt: VideoFormat) -> &'static str {
    match fmt {
        VideoFormat::Mp4 => "mp4",
        VideoFormat::Mkv => "matroska",
        VideoFormat::Webm => "webm",
        VideoFormat::Mov => "mov",
        VideoFormat::Avi => "avi",
    }
}

fn missing_encoder(what: &str) -> Error {
    Error::External { program: "ffmpeg", message: format!("this FFmpeg build has no {what} encoder") }
}

/// Encoder arguments for the (single) video stream.
fn video_encoder(fmt: VideoFormat, quality: VideoQuality, out_height: u32, enc: &HashSet<String>) -> Result<Vec<String>> {
    let level = match quality {
        VideoQuality::Original | VideoQuality::High => 0,
        VideoQuality::Medium => 1,
        VideoQuality::Small => 2,
    };
    // Rough bitrate for encoders without a constant-quality mode: ~8 Mbit/s at 1080p for High.
    let h = if out_height == 0 { 1080.0 } else { out_height as f64 };
    let kbps = ((h * h * [0.0069, 0.0043, 0.0021][level]) as u32).max(300);
    let candidates: &[&str] = match fmt {
        VideoFormat::Mp4 | VideoFormat::Mov | VideoFormat::Mkv => &["libx264", "libopenh264", "mpeg4"],
        VideoFormat::Webm => &["libvpx-vp9", "libvpx"],
        VideoFormat::Avi => &["mpeg4"],
    };
    let Some(name) = candidates.iter().find(|c| enc.contains(**c)) else {
        return Err(missing_encoder(if fmt == VideoFormat::Webm { "VP9/VP8 (libvpx)" } else { "suitable video" }));
    };
    let a: Vec<String> = match *name {
        "libx264" => vec!["-preset".into(), "medium".into(), "-crf".into(), ["18", "23", "28"][level].into()],
        "libopenh264" => vec!["-b:v".into(), format!("{kbps}k")],
        "libvpx-vp9" => vec![
            "-crf".into(), ["24", "32", "40"][level].into(), "-b:v".into(), "0".into(),
            "-deadline".into(), "good".into(), "-cpu-used".into(), "4".into(), "-row-mt".into(), "1".into(),
        ],
        "libvpx" => vec!["-crf".into(), ["8", "16", "28"][level].into(), "-b:v".into(), format!("{}k", kbps * 2)],
        _ => {
            let mut a = vec!["-q:v".to_string(), ["2", "4", "8"][level].into()];
            if fmt == VideoFormat::Avi {
                a.extend(["-vtag".into(), "XVID".into()]);
            }
            a
        }
    };
    let mut out = vec!["-c:v".to_string(), name.to_string()];
    out.extend(a);
    out.extend(["-pix_fmt".into(), "yuv420p".into()]);
    Ok(out)
}

/// Encoder for one audio stream at output index `i`.
fn audio_encoder(fmt: VideoFormat, i: usize, channels: u32, enc: &HashSet<String>) -> Result<Vec<String>> {
    let (name, rate, max_channels) = match fmt {
        VideoFormat::Mp4 | VideoFormat::Mov | VideoFormat::Mkv => ("aac", Some("192k"), 8),
        VideoFormat::Webm if enc.contains("libopus") => ("libopus", Some("128k"), 2),
        VideoFormat::Webm if enc.contains("libvorbis") => ("libvorbis", None, 8),
        VideoFormat::Webm => return Err(missing_encoder("Opus or Vorbis")),
        VideoFormat::Avi if enc.contains("libmp3lame") => ("libmp3lame", Some("192k"), 2),
        VideoFormat::Avi => ("ac3", Some("192k"), 6),
    };
    let mut a = vec![format!("-c:{i}"), name.to_string()];
    match rate {
        Some(r) => a.extend([format!("-b:{i}"), r.to_string()]),
        None => a.extend([format!("-q:{i}"), "5".to_string()]),
    }
    if channels > max_channels {
        a.extend([format!("-ac:{i}"), "2".to_string()]);
    }
    Ok(a)
}

fn plan_video(o: &ConvertVideoOptions, p: &Probe, enc: &HashSet<String>, name: &str) -> Result<Plan> {
    let fmt = o.format;
    let video = p.of(Kind::Video).next().ok_or_else(|| Error::InvalidOptions(format!("{name} has no video picture")))?;
    let mut plan = Plan::default();
    let mut copied = true;
    let mut i = 0usize;

    let scale_to = o.resolution.max_height().filter(|&h| video.height > h);
    plan.args.extend(["-map".into(), format!("0:{}", video.index)]);
    if o.quality == VideoQuality::Original && scale_to.is_none() && video_fits(fmt, &video.codec) {
        plan.args.extend([format!("-c:{i}"), "copy".into()]);
        if matches!(fmt, VideoFormat::Mp4 | VideoFormat::Mov) && video.codec == "hevc" {
            // Apple players only accept HEVC tagged as hvc1.
            plan.args.extend([format!("-tag:{i}"), "hvc1".into()]);
        }
    } else {
        copied = false;
        plan.args.extend(video_encoder(fmt, o.quality, scale_to.unwrap_or(video.height), enc)?);
        // yuv420p needs even dimensions; -2 keeps the aspect ratio with an even width.
        let filter = match scale_to {
            Some(h) => format!("scale=-2:{h}"),
            None => "scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string(),
        };
        plan.args.extend(["-vf".into(), filter]);
    }
    i += 1;

    if !o.remove_audio {
        let take = if fmt == VideoFormat::Avi { 1 } else { usize::MAX };
        for a in p.of(Kind::Audio).take(take) {
            plan.args.extend(["-map".into(), format!("0:{}", a.index)]);
            if audio_fits(fmt, &a.codec) {
                plan.args.extend([format!("-c:{i}"), "copy".into()]);
            } else {
                copied = false;
                plan.args.extend(audio_encoder(fmt, i, a.channels, enc)?);
            }
            i += 1;
        }
    }

    let mut dropped = 0;
    for s in p.of(Kind::Subtitle) {
        let codec = match fmt {
            VideoFormat::Mkv if s.codec == "mov_text" => "srt",
            VideoFormat::Mkv => "copy",
            VideoFormat::Mp4 | VideoFormat::Mov if is_text_subtitle(&s.codec) => "mov_text",
            VideoFormat::Webm if is_text_subtitle(&s.codec) => "webvtt",
            _ => {
                dropped += 1;
                continue;
            }
        };
        plan.args.extend(["-map".into(), format!("0:{}", s.index), format!("-c:{i}"), codec.into()]);
        i += 1;
    }
    if dropped > 0 {
        plan.notes.push(format!(
            "{name}: {dropped} subtitle track{} left out, {} cannot hold {}",
            if dropped == 1 { "" } else { "s" },
            format_label(fmt),
            if fmt == VideoFormat::Avi { "subtitles" } else { "picture-based subtitles" },
        ));
    }
    if fmt == VideoFormat::Mkv {
        // Fonts attached for styled subtitles.
        plan.args.extend(["-map".into(), "0:t?".into(), "-c:t".into(), "copy".into()]);
    }
    if matches!(fmt, VideoFormat::Mp4 | VideoFormat::Mov) {
        plan.args.extend(["-movflags".into(), "+faststart".into()]);
    }
    if copied {
        plan.notes.push(format!("{name}: streams copied unchanged, no quality lost"));
    }
    Ok(plan)
}

fn plan_audio(format: AudioFormat, p: &Probe, enc: &HashSet<String>, name: &str) -> Result<Plan> {
    let a = p.of(Kind::Audio).next().ok_or_else(|| Error::InvalidOptions(format!("{name} has no sound")))?;
    let mut plan = Plan { args: vec!["-map".into(), format!("0:{}", a.index)], notes: Vec::new() };
    let fits = matches!(
        (format, a.codec.as_str()),
        (AudioFormat::Mp3, "mp3") | (AudioFormat::M4a, "aac") | (AudioFormat::Flac, "flac") | (AudioFormat::Opus, "opus") | (AudioFormat::Wav, "pcm_s16le")
    );
    if fits {
        plan.args.extend(["-c:a".into(), "copy".into()]);
        plan.notes.push(format!("{name}: sound copied unchanged, no quality lost"));
    } else {
        let (codec, rate, stereo) = match format {
            AudioFormat::Mp3 if enc.contains("libmp3lame") => ("libmp3lame", Some("192k"), true),
            AudioFormat::Mp3 => return Err(missing_encoder("MP3 (libmp3lame)")),
            AudioFormat::M4a => ("aac", Some("192k"), false),
            AudioFormat::Wav => ("pcm_s16le", None, false),
            AudioFormat::Flac => ("flac", None, false),
            AudioFormat::Opus if enc.contains("libopus") => ("libopus", Some("128k"), true),
            AudioFormat::Opus => return Err(missing_encoder("Opus (libopus)")),
        };
        plan.args.extend(["-c:a".into(), codec.into()]);
        if let Some(r) = rate {
            plan.args.extend(["-b:a".into(), r.into()]);
        }
        if stereo && a.channels > 2 {
            plan.args.extend(["-ac".into(), "2".into()]);
        }
    }
    if format == AudioFormat::M4a {
        plan.args.extend(["-movflags".into(), "+faststart".into()]);
    }
    Ok(plan)
}

fn audio_muxer(format: AudioFormat) -> &'static str {
    match format {
        AudioFormat::Mp3 => "mp3",
        AudioFormat::M4a => "ipod",
        AudioFormat::Wav => "wav",
        AudioFormat::Flac => "flac",
        AudioFormat::Opus => "opus",
    }
}

/// Runs ffmpeg for input `i` of `n` into a new output file, reporting progress from `-progress`.
fn transcode(ctx: &mut Ctx<'_>, i: usize, n: usize, plan: &Plan, duration: Option<f64>, mux: &str, ext: &str) -> Result<()> {
    let input = ctx.input_path(i).to_path_buf();
    let name = ctx.name(i);
    let stem = ctx.stem_n(i, n);
    let (progress, cancel) = (ctx.progress, ctx.cancel);
    let msg = format!("Converting {name}");
    ctx.emit(&stem, ext, |tmp| {
        let mut args: Vec<OsString> =
            ["-hide_banner", "-nostdin", "-nostats", "-loglevel", "error", "-progress", "pipe:1", "-y", "-i"].iter().map(os).collect();
        args.push(file_arg(&input));
        args.extend(plan.args.iter().map(os));
        args.extend([os("-max_muxing_queue_size"), os("4096"), os("-f"), os(mux), file_arg(tmp)]);
        let mut on_line = |line: &str| {
            let t = line.strip_prefix("out_time_us=").or_else(|| line.strip_prefix("out_time_ms="));
            if let (Some(us), Some(d)) = (t.and_then(|t| t.trim().parse::<f64>().ok()), duration) {
                let done = (us / 1e6 / d).clamp(0.0, 1.0);
                progress(((i as f64 + done) / n as f64) as f32, &msg);
            }
        };
        let out = run_cmd_lines("ffmpeg", &args, cancel, &mut on_line)?;
        if !out.success {
            return Err(Error::External { program: "ffmpeg", message: out.message() });
        }
        if file_size(tmp) == 0 {
            return Err(Error::External { program: "ffmpeg", message: format!("{name} produced no output") });
        }
        Ok(())
    })?;
    Ok(())
}

pub(super) fn convert_video(ctx: &mut Ctx<'_>, o: &ConvertVideoOptions) -> Result<()> {
    let n = ctx.req.inputs.len();
    let enc = encoders(ctx.cancel)?;
    for i in 0..n {
        ctx.check()?;
        let name = ctx.name(i);
        ctx.step(i, n, &format!("Reading {name}"));
        let p = probe(ctx.input_path(i), &name, ctx.cancel)?;
        let plan = plan_video(o, &p, &enc, &name)?;
        transcode(ctx, i, n, &plan, p.duration, muxer(o.format), o.format.ext())?;
        ctx.notes.extend(plan.notes);
    }
    Ok(())
}

pub(super) fn video_to_audio(ctx: &mut Ctx<'_>, format: AudioFormat) -> Result<()> {
    let n = ctx.req.inputs.len();
    let enc = encoders(ctx.cancel)?;
    for i in 0..n {
        ctx.check()?;
        let name = ctx.name(i);
        ctx.step(i, n, &format!("Reading {name}"));
        let p = probe(ctx.input_path(i), &name, ctx.cancel)?;
        let plan = plan_audio(format, &p, &enc, &name)?;
        transcode(ctx, i, n, &plan, p.duration, audio_muxer(format), format.ext())?;
        ctx.notes.extend(plan.notes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VideoResolution;

    const MKV_PROBE: &str = r#"{
        "streams": [
            { "index": 0, "codec_name": "h264", "codec_type": "video", "height": 1080, "disposition": { "attached_pic": 0 } },
            { "index": 1, "codec_name": "aac", "codec_type": "audio", "channels": 2, "disposition": { "attached_pic": 0 } },
            { "index": 2, "codec_name": "flac", "codec_type": "audio", "channels": 6, "disposition": { "attached_pic": 0 } },
            { "index": 3, "codec_name": "subrip", "codec_type": "subtitle", "disposition": { "attached_pic": 0 } },
            { "index": 4, "codec_name": "hdmv_pgs_subtitle", "codec_type": "subtitle", "disposition": { "attached_pic": 0 } },
            { "index": 5, "codec_name": "mjpeg", "codec_type": "video", "height": 600, "disposition": { "attached_pic": 1 } }
        ],
        "format": { "duration": "62.500000" }
    }"#;

    fn enc(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn full() -> HashSet<String> {
        enc(&["libx264", "mpeg4", "libvpx-vp9", "libvpx", "aac", "libopus", "libvorbis", "libmp3lame", "ac3", "flac", "pcm_s16le"])
    }

    fn opts(format: VideoFormat) -> ConvertVideoOptions {
        ConvertVideoOptions { format, ..Default::default() }
    }

    fn joined(p: &Plan) -> String {
        p.args.join(" ")
    }

    #[test]
    fn probe_json_is_parsed() {
        let p = parse_probe(MKV_PROBE).unwrap();
        assert_eq!(p.duration, Some(62.5));
        assert_eq!(p.streams.len(), 6);
        assert!(p.streams[5].cover);
        assert_eq!(p.of(Kind::Video).count(), 1, "cover art is not a video");
        assert_eq!(p.streams[2].channels, 6);
        assert!(parse_probe("not json").is_none());
        assert_eq!(parse_probe(r#"{"streams": [], "format": {"duration": "N/A"}}"#).unwrap().duration, None);
    }

    #[test]
    fn encoder_list_is_parsed() {
        let text = "Encoders:\n V..... = Video\n A..... = Audio\n ------\n V....D libx264              libx264 H.264\n A....D aac                  AAC (Advanced Audio Coding)\n S..... mov_text             3GPP Timed Text subtitle\n";
        assert_eq!(parse_encoders(text), enc(&["libx264", "aac", "mov_text"]));
    }

    #[test]
    fn mkv_to_mp4_copies_what_fits() {
        let p = parse_probe(MKV_PROBE).unwrap();
        let plan = plan_video(&opts(VideoFormat::Mp4), &p, &full(), "a").unwrap();
        let a = joined(&plan);
        assert!(a.starts_with("-map 0:0 -c:0 copy -map 0:1 -c:1 copy -map 0:2 -c:2 aac -b:2 192k"), "{a}");
        assert!(a.contains("-map 0:3 -c:3 mov_text"), "{a}");
        assert!(!a.contains("0:4") && !a.contains("0:5"), "picture subtitles and cover art stay out: {a}");
        assert!(a.contains("-movflags +faststart"));
        assert!(plan.notes.iter().any(|n| n.contains("1 subtitle track left out")), "{:?}", plan.notes);
        assert!(!plan.notes.iter().any(|n| n.contains("copied unchanged")), "audio was re-encoded");
    }

    #[test]
    fn mkv_target_copies_everything() {
        let p = parse_probe(MKV_PROBE).unwrap();
        let plan = plan_video(&opts(VideoFormat::Mkv), &p, &full(), "a").unwrap();
        let a = joined(&plan);
        assert!(!a.contains("libx264") && !a.contains("aac"), "{a}");
        assert!(a.contains("-map 0:4 -c:4 copy") && a.contains("-map 0:t?"), "{a}");
        assert!(plan.notes.iter().any(|n| n.contains("copied unchanged")));
    }

    #[test]
    fn webm_reencodes_and_downmixes_opus() {
        let p = parse_probe(MKV_PROBE).unwrap();
        let plan = plan_video(&opts(VideoFormat::Webm), &p, &full(), "a").unwrap();
        let a = joined(&plan);
        assert!(a.contains("-c:v libvpx-vp9 -crf 24 -b:v 0"), "{a}");
        assert!(a.contains("-c:1 libopus -b:1 128k") && a.contains("-c:2 libopus -b:2 128k -ac:2 2"), "{a}");
        assert!(a.contains("-c:3 webvtt"), "{a}");
        // Fedora-style builds without libvpx cannot make WebM.
        let err = plan_video(&opts(VideoFormat::Webm), &p, &enc(&["aac", "libopus"]), "a").unwrap_err();
        assert!(err.to_string().contains("libvpx"), "{err}");
    }

    #[test]
    fn avi_keeps_one_audio_track_and_no_subtitles() {
        let p = parse_probe(MKV_PROBE).unwrap();
        let plan = plan_video(&opts(VideoFormat::Avi), &p, &full(), "a").unwrap();
        let a = joined(&plan);
        assert!(a.contains("-c:v mpeg4 -q:v 2 -vtag XVID"), "{a}");
        assert!(a.contains("-map 0:1 -c:1 libmp3lame") && !a.contains("0:2"), "{a}");
        assert!(plan.notes.iter().any(|n| n.contains("2 subtitle tracks left out")), "{:?}", plan.notes);
    }

    #[test]
    fn resolution_and_quality_force_reencode_but_never_upscale() {
        let p = parse_probe(MKV_PROBE).unwrap();
        let o = ConvertVideoOptions { resolution: VideoResolution::P720, ..opts(VideoFormat::Mp4) };
        let a = joined(&plan_video(&o, &p, &full(), "a").unwrap());
        assert!(a.contains("-c:v libx264") && a.contains("-vf scale=-2:720"), "{a}");

        let o = ConvertVideoOptions { resolution: VideoResolution::P1080, ..opts(VideoFormat::Mp4) };
        let a = joined(&plan_video(&o, &p, &full(), "a").unwrap());
        assert!(a.starts_with("-map 0:0 -c:0 copy"), "already 1080p: {a}");

        let o = ConvertVideoOptions { quality: VideoQuality::Small, remove_audio: true, ..opts(VideoFormat::Mp4) };
        let a = joined(&plan_video(&o, &p, &full(), "a").unwrap());
        assert!(a.contains("-crf 28") && a.contains("scale=trunc(iw/2)*2"), "{a}");
        assert!(!a.contains("0:1") && !a.contains("0:2"), "sound removed: {a}");
    }

    #[test]
    fn falls_back_to_other_h264_encoders() {
        let p = parse_probe(MKV_PROBE).unwrap();
        let o = ConvertVideoOptions { quality: VideoQuality::Medium, ..opts(VideoFormat::Mp4) };
        let a = joined(&plan_video(&o, &p, &enc(&["libopenh264", "aac"]), "a").unwrap());
        assert!(a.contains("-c:v libopenh264 -b:v 5015k"), "{a}");
        let a = joined(&plan_video(&o, &p, &enc(&["mpeg4", "aac"]), "a").unwrap());
        assert!(a.contains("-c:v mpeg4 -q:v 4") && !a.contains("XVID"), "{a}");
    }

    #[test]
    fn hevc_in_mp4_is_tagged_for_apple_players() {
        let p = parse_probe(&MKV_PROBE.replace("\"h264\"", "\"hevc\"")).unwrap();
        let a = joined(&plan_video(&opts(VideoFormat::Mp4), &p, &full(), "a").unwrap());
        assert!(a.contains("-c:0 copy -tag:0 hvc1"), "{a}");
    }

    #[test]
    fn audio_only_and_no_video() {
        let p = parse_probe(r#"{"streams": [{"index": 0, "codec_name": "aac", "codec_type": "audio", "channels": 2}], "format": {}}"#).unwrap();
        let err = plan_video(&opts(VideoFormat::Mp4), &p, &full(), "clip").unwrap_err();
        assert!(err.to_string().contains("clip has no video"), "{err}");

        let plan = plan_audio(AudioFormat::M4a, &p, &full(), "clip").unwrap();
        assert_eq!(joined(&plan), "-map 0:0 -c:a copy -movflags +faststart");
        let plan = plan_audio(AudioFormat::Mp3, &p, &full(), "clip").unwrap();
        assert_eq!(joined(&plan), "-map 0:0 -c:a libmp3lame -b:a 192k");
        assert!(plan_audio(AudioFormat::Mp3, &p, &enc(&["aac"]), "clip").is_err());
    }

    #[test]
    fn audio_from_video_without_sound_fails() {
        let p = parse_probe(r#"{"streams": [{"index": 0, "codec_name": "h264", "codec_type": "video", "height": 240}]}"#).unwrap();
        let err = plan_audio(AudioFormat::Wav, &p, &full(), "silent").unwrap_err();
        assert!(err.to_string().contains("silent has no sound"), "{err}");
        let p = parse_probe(MKV_PROBE).unwrap();
        assert_eq!(joined(&plan_audio(AudioFormat::Opus, &p, &full(), "a").unwrap()), "-map 0:1 -c:a libopus -b:a 128k");
    }
}
