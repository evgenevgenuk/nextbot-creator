//! Builds the GMod addon folder: lua/entities/npc_<name>.lua, sound/<name>/, materials/<name>/.

use anyhow::{bail, Context, Result};
use image::codecs::gif::GifDecoder;
use image::AnimationDecoder;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

const TEMPLATE: &str = include_str!("nextbot_template.lua");

pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif"];
pub const SOUND_EXTS: &[&str] = &["mp3", "wav", "ogg"];

pub struct NextbotSpec {
    pub name: String,
    pub image: PathBuf,
    pub chase_sound: PathBuf,
    pub jump_sound: PathBuf,
    pub attack_sound: PathBuf,
    pub physgun_disabled: bool,
    pub admin_only: bool,
}

pub fn ext_lower(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn is_gif(path: &Path) -> bool {
    ext_lower(path) == "gif"
}

/// Same rule as the original tool: keep latin letters only, lowercase.
pub fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// One decoded GIF frame (full canvas, RGBA) plus how long it stays on screen.
pub struct GifFrame {
    pub image: image::RgbaImage,
    pub delay_secs: f32,
}

pub fn decode_gif(path: &Path) -> Result<Vec<GifFrame>> {
    let file = fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let decoder = GifDecoder::new(BufReader::new(file)).context("not a valid GIF")?;
    let frames = decoder.into_frames().collect_frames().context("cannot decode GIF frames")?;
    if frames.is_empty() {
        bail!("GIF has no frames");
    }
    Ok(frames
        .into_iter()
        .map(|f| {
            let (num, den) = f.delay().numer_denom_ms();
            let ms = if den == 0 { 0.0 } else { num as f32 / den as f32 };
            // Browsers show 0-10 ms delays as 100 ms; do the same.
            let ms = if ms < 20.0 { 100.0 } else { ms };
            GifFrame { image: f.into_buffer(), delay_secs: ms / 1000.0 }
        })
        .collect())
}

fn lua_string_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ").replace('\r', "")
}

fn render_code(name: &str, image_file: &str, gif_delays: Option<&[f32]>) -> String {
    let mut s = String::new();
    match gif_delays {
        None => {
            s += &format!("local {name}Material = Material(\"{name}/{image_file}\")\n");
            s += "local drawOffset = Vector(0, 0, 64)\n";
            s += "function ENT:RenderOverride()\n";
            s += &format!("\trender.SetMaterial({name}Material)\n");
            s += "\trender.DrawSprite(self:GetPos() + drawOffset, 128, 128)\n";
            s += "end\n";
        }
        Some(delays) => {
            let list: Vec<String> = delays.iter().map(|d| format!("{}", (d * 1000.0).round() / 1000.0)).collect();
            s += &format!("-- Animated GIF: frames extracted to materials/{name}/frames/\n");
            s += "local gifFrames = {}\n";
            s += &format!("for i = 1, {} do\n", delays.len());
            s += &format!("\tgifFrames[i] = Material(\"{name}/frames/frame\" .. i .. \".png\")\n");
            s += "end\n";
            s += &format!("local gifDelays = {{{}}}\n", list.join(", "));
            s += "local gifTotal = 0\n";
            s += "for i = 1, #gifDelays do gifTotal = gifTotal + gifDelays[i] end\n";
            s += "local drawOffset = Vector(0, 0, 64)\n";
            s += "function ENT:RenderOverride()\n";
            s += "\tlocal t = RealTime() % gifTotal\n";
            s += "\tlocal frame = #gifFrames\n";
            s += "\tfor i = 1, #gifDelays do\n";
            s += "\t\tt = t - gifDelays[i]\n";
            s += "\t\tif (t < 0) then frame = i break end\n";
            s += "\tend\n";
            s += "\trender.SetMaterial(gifFrames[frame])\n";
            s += "\trender.DrawSprite(self:GetPos() + drawOffset, 128, 128)\n";
            s += "end\n";
        }
    }
    s
}

fn copy_as(src: &Path, dir: &Path, stem: &str) -> Result<String> {
    let file_name = format!("{stem}.{}", ext_lower(src));
    fs::copy(src, dir.join(&file_name))
        .with_context(|| format!("cannot copy {}", src.display()))?;
    Ok(file_name)
}

/// Creates `<out_root>/<cleanname>/...` and returns that addon folder.
pub fn create_nextbot(spec: &NextbotSpec, out_root: &Path) -> Result<PathBuf> {
    let name = clean_name(&spec.name);
    if name.is_empty() {
        bail!("The name must contain at least one latin letter (a-z).");
    }
    for (label, path, exts) in [
        ("Image", &spec.image, IMAGE_EXTS),
        ("Chase sound", &spec.chase_sound, SOUND_EXTS),
        ("Jump sound", &spec.jump_sound, SOUND_EXTS),
        ("Attack sound", &spec.attack_sound, SOUND_EXTS),
    ] {
        if !path.is_file() {
            bail!("{label}: file not found ({})", path.display());
        }
        if !exts.contains(&ext_lower(path).as_str()) {
            bail!("{label}: unsupported file type .{} (use {})", ext_lower(path), exts.join(", "));
        }
    }

    let addon = out_root.join(&name);
    let lua_dir = addon.join("lua").join("entities");
    let sound_dir = addon.join("sound").join(&name);
    let mat_dir = addon.join("materials").join(&name);
    for d in [&lua_dir, &sound_dir, &mat_dir] {
        fs::create_dir_all(d).with_context(|| format!("cannot create {}", d.display()))?;
    }

    let chase = copy_as(&spec.chase_sound, &sound_dir, "chasesound")?;
    let jump = copy_as(&spec.jump_sound, &sound_dir, "jumpsound")?;
    let attack = copy_as(&spec.attack_sound, &sound_dir, "attacksound")?;

    // Leftovers from an earlier build with a different image type would confuse GMod.
    let frames_dir = mat_dir.join("frames");
    if frames_dir.exists() {
        fs::remove_dir_all(&frames_dir)?;
    }

    let render = if is_gif(&spec.image) {
        // GMod cannot load .gif materials: save every frame as frameN.png.
        let frames = decode_gif(&spec.image)?;
        fs::create_dir_all(&frames_dir)?;
        for (i, f) in frames.iter().enumerate() {
            f.image
                .save(frames_dir.join(format!("frame{}.png", i + 1)))
                .context("cannot save GIF frame")?;
        }
        let delays: Vec<f32> = frames.iter().map(|f| f.delay_secs).collect();
        render_code(&name, "", Some(&delays))
    } else {
        let image_file = copy_as(&spec.image, &mat_dir, "image")?;
        render_code(&name, &image_file, None)
    };

    let bool_str = |b: bool| if b { "true" } else { "false" };
    let lua = TEMPLATE
        .replace("@@RENDER@@", &render)
        .replace("@@PHYSGUN_DISABLED@@", bool_str(spec.physgun_disabled))
        .replace("@@ADMIN_ONLY@@", bool_str(spec.admin_only))
        .replace("@@DISPLAY_NAME@@", &lua_string_escape(&spec.name))
        .replace("@@CHASE@@", &chase)
        .replace("@@JUMP@@", &jump)
        .replace("@@ATTACK@@", &attack)
        .replace("@@NAME@@", &name);
    fs::write(lua_dir.join(format!("npc_{name}.lua")), lua.replace('\n', "\r\n"))
        .context("cannot write Lua file")?;
    Ok(addon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, Rgba, RgbaImage};

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nbc_test_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_test_gif(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut enc = GifEncoder::new(file);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for (i, ms) in [(0u8, 50u32), (1, 120), (2, 0)] {
            let img = RgbaImage::from_pixel(16, 16, Rgba([i * 100, 255 - i * 100, 0, 255]));
            enc.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(ms, 1))).unwrap();
        }
    }

    fn spec(dir: &Path, image: &str) -> NextbotSpec {
        for s in ["a.mp3", "b.wav", "c.ogg"] {
            fs::write(dir.join(s), b"x").unwrap();
        }
        NextbotSpec {
            name: "Test \"Bot\" 2".into(),
            image: dir.join(image),
            chase_sound: dir.join("a.mp3"),
            jump_sound: dir.join("b.wav"),
            attack_sound: dir.join("c.ogg"),
            physgun_disabled: true,
            admin_only: false,
        }
    }

    #[test]
    fn gif_becomes_png_frames() {
        let dir = temp_dir("gif");
        write_test_gif(&dir.join("anim.gif"));
        let addon = create_nextbot(&spec(&dir, "anim.gif"), &dir.join("out")).unwrap();
        assert!(addon.ends_with("testbot"));
        for i in 1..=3 {
            assert!(addon.join(format!("materials/testbot/frames/frame{i}.png")).is_file());
        }
        let lua = fs::read_to_string(addon.join("lua/entities/npc_testbot.lua")).unwrap();
        assert!(lua.contains("local gifDelays = {0.05, 0.12, 0.1}"), "delays wrong");
        assert!(lua.contains("Material(\"testbot/frames/frame\" .. i .. \".png\")"));
        assert!(lua.contains("ENT.PhysgunDisabled = true"));
        assert!(lua.contains("AdminOnly = false"));
        assert!(lua.contains("Name = \"Test \\\"Bot\\\" 2\""));
        // Each sound keeps its own extension.
        assert!(lua.contains("\"testbot/chasesound.mp3\""));
        assert!(lua.contains("\"testbot/jumpsound.wav\""));
        assert!(lua.contains("\"testbot/attacksound.ogg\""));
        assert!(!lua.contains("@@"), "unreplaced placeholder");
    }

    #[test]
    fn png_is_copied_as_before() {
        let dir = temp_dir("png");
        RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255])).save(dir.join("pic.png")).unwrap();
        let addon = create_nextbot(&spec(&dir, "pic.png"), &dir.join("out")).unwrap();
        assert!(addon.join("materials/testbot/image.png").is_file());
        assert!(!addon.join("materials/testbot/frames").exists());
        let lua = fs::read_to_string(addon.join("lua/entities/npc_testbot.lua")).unwrap();
        assert!(lua.contains("Material(\"testbot/image.png\")"));
    }

    #[test]
    fn name_without_latin_letters_is_rejected() {
        let dir = temp_dir("name");
        write_test_gif(&dir.join("anim.gif"));
        let mut s = spec(&dir, "anim.gif");
        s.name = "Бот 123".into();
        assert!(create_nextbot(&s, &dir.join("out")).is_err());
    }
}
