//! Renders `crates/app/ui/app.slint` with fake data to PNG files.
//!
//! cargo run --manifest-path tools/ui-snapshot/Cargo.toml -- <out-dir>

use std::rc::Rc;

use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType, TargetPixel,
};
use slint::platform::{Platform, WindowAdapter};
use slint::{
    ComponentHandle, Image, ModelRc, PhysicalSize, Rgba8Pixel, SharedPixelBuffer, SharedString,
    VecModel,
};
use slint_interpreter::{Compiler, ComponentInstance, Struct, Value};

#[derive(Clone, Copy, Default)]
struct Px(Rgba8Pixel);

impl TargetPixel for Px {
    fn blend(&mut self, c: PremultipliedRgbaColor) {
        let a = u16::from(255 - c.alpha);
        let mix = |dst: u8, src: u8| (u16::from(src) + u16::from(dst) * a / 255) as u8;
        self.0.r = mix(self.0.r, c.red);
        self.0.g = mix(self.0.g, c.green);
        self.0.b = mix(self.0.b, c.blue);
        self.0.a = 255;
    }

    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Px(Rgba8Pixel { r, g, b, a: 255 })
    }
}

struct SnapshotPlatform(Rc<MinimalSoftwareWindow>);

impl Platform for SnapshotPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn s(v: &str) -> Value {
    Value::String(SharedString::from(v))
}

fn cover(seed: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(64, 64);
    for (i, p) in buf.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = ((i % 64) as u32, (i / 64) as u32);
        *p = Rgba8Pixel {
            r: ((x * 3 + seed * 40) % 256) as u8,
            g: ((y * 2 + seed * 70) % 256) as u8,
            b: (160 + seed * 13 % 90) as u8,
            a: 255,
        };
    }
    Image::from_rgba8(buf)
}

fn model(values: Vec<Value>) -> Value {
    Value::Model(ModelRc::new(VecModel::from(values)))
}

fn row(
    id: f64,
    index: f64,
    title: &str,
    subtitle: &str,
    meta: &str,
    artwork: &str,
    playable: bool,
    cover: Image,
) -> Struct {
    let mut row = Struct::default();
    row.set_field("kind".into(), Value::Number(if playable { 1.0 } else { 0.0 }));
    row.set_field("id".into(), Value::Number(id));
    row.set_field("index".into(), Value::Number(index));
    row.set_field("title".into(), s(title));
    row.set_field("subtitle".into(), s(subtitle));
    row.set_field("meta".into(), s(meta));
    row.set_field("artwork".into(), s(artwork));
    row.set_field("playable".into(), Value::Bool(playable));
    row.set_field("favorite".into(), Value::Bool(false));
    row.set_field("cover".into(), Value::Image(cover));
    row
}

fn cards(columns: usize) -> Value {
    let titles = [
        ("ハルジオン", "YOASOBI", "2020"),
        ("我的歌声里", "曲婉婷", "2012"),
        (
            "Night Flight with a Very Long Album Title",
            "Various Artists",
            "1999",
        ),
        ("君の名は。", "RADWIMPS", "2016"),
        ("Random Access Memories", "Daft Punk", "2013"),
        ("十一月的萧邦", "周杰伦", "2005"),
        ("Untitled", "", ""),
        ("CLANNAD ORIGINAL SOUNDTRACK", "麻枝准 / 戸越まごめ", "2004"),
    ];
    let cards: Vec<Value> = (0..24)
        .map(|i| {
            let (title, artist, year) = titles[i % titles.len()];
            Value::Struct(row(
                i as f64,
                i as f64,
                title,
                artist,
                year,
                "/api/v1/artwork/x",
                false,
                if i % 5 == 4 {
                    Image::default()
                } else {
                    cover(i as u32)
                },
            ))
        })
        .collect();
    model(
        cards
            .chunks(columns)
            .map(|chunk| {
                let mut row = Struct::default();
                row.set_field("items".into(), model(chunk.to_vec()));
                Value::Struct(row)
            })
            .collect(),
    )
}

fn lines() -> Value {
    let rows = [
        ("夜に駆ける", "YOASOBI", "THE BOOK · 4:21"),
        ("晴天", "周杰伦", "叶惠美 · 4:29"),
        (
            "Get Lucky (feat. Pharrell Williams & Nile Rodgers)",
            "Daft Punk",
            "Random Access Memories · 6:09",
        ),
        ("だんご大家族", "茶太", "CLANNAD ORIGINAL SOUNDTRACK · 3:38"),
    ];
    model(
        (0..40)
            .map(|i| {
                let (t, a, m) = rows[i % rows.len()];
                Value::Struct(row(
                    100.0 + i as f64,
                    i as f64,
                    t,
                    a,
                    m,
                    "",
                    true,
                    cover(i as u32),
                ))
            })
            .collect(),
    )
}

fn toolbar_with(filterable: bool) -> Value {
    let mut t = Struct::default();
    // Detail pages have no sort menu.
    let sorts: Vec<Value> = if filterable {
        ["最近添加", "名称", "最近播放", "发行日期", "发行年份", "随机"]
            .into_iter()
            .map(s)
            .collect()
    } else {
        Vec::new()
    };
    t.set_field("sorts".into(), model(sorts));
    t.set_field("sort-index".into(), Value::Number(0.0));
    t.set_field("desc".into(), Value::Bool(true));
    t.set_field("search".into(), s(""));
    t.set_field("year-from".into(), Value::Number(0.0));
    t.set_field("year-to".into(), Value::Number(0.0));
    t.set_field("favorite".into(), Value::Number(0.0));
    t.set_field("filterable".into(), Value::Bool(filterable));
    Value::Struct(t)
}

fn queue() -> Value {
    let rows = [
        ("夜に駆ける", "YOASOBI", "4:21"),
        ("晴天", "周杰伦", "4:29"),
        ("Get Lucky (feat. Pharrell Williams & Nile Rodgers)", "Daft Punk", "6:09"),
        ("だんご大家族", "茶太", "3:38"),
    ];
    model(
        (0..12)
            .map(|i| {
                let (t, a, d) = rows[i % rows.len()];
                let mut row = Struct::default();
                row.set_field("uid".into(), Value::Number(i as f64 + 1.0));
                row.set_field("title".into(), s(t));
                row.set_field("artist".into(), s(a));
                row.set_field("duration".into(), s(d));
                Value::Struct(row)
            })
            .collect(),
    )
}

fn lyrics() -> Value {
    let lines = [
        "沈むように溶けてゆくように",
        "二人だけの空が広がる夜に",
        "「さよなら」だけだった",
        "その一言で全てが分かった",
        "日が沈み出した空と君の姿",
        "フェンス越しに重なっていた",
        "初めて会った日から",
        "僕の心の全てを奪った",
    ];
    model(
        lines
            .iter()
            .map(|l| {
                let mut row = Struct::default();
                row.set_field("text".into(), s(l));
                Value::Struct(row)
            })
            .collect(),
    )
}

fn set(inst: &ComponentInstance, name: &str, value: Value) {
    inst.set_property(name, value)
        .unwrap_or_else(|e| panic!("set {name}: {e:?}"));
}

fn render(window: &MinimalSoftwareWindow, size: PhysicalSize, path: &str) {
    window.set_size(size);
    // Let layouts, `init` and animations settle.
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|_| {});
    }
    std::thread::sleep(std::time::Duration::from_millis(450));
    slint::platform::update_timers_and_animations();
    window.request_redraw();
    let (w, h) = (size.width as usize, size.height as usize);
    let mut buf = vec![Px::default(); w * h];
    window.draw_if_needed(|renderer| {
        renderer.render(&mut buf, w);
    });
    let bytes: Vec<u8> = buf
        .iter()
        .flat_map(|p| [p.0.r, p.0.g, p.0.b, 255])
        .collect();
    image::RgbaImage::from_raw(size.width, size.height, bytes)
        .unwrap()
        .save(path)
        .unwrap();
    println!("wrote {path}");
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "/tmp".into());
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/app/ui/app.slint");

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SnapshotPlatform(window.clone()))).unwrap();

    let mut compiler = Compiler::default();
    compiler.set_style("fluent-dark".into());
    let result = spin_on::spin_on(compiler.build_from_path(root));
    for d in result.diagnostics() {
        eprintln!("{d}");
    }
    let def = result.component("AppWindow").expect("AppWindow");
    let size = PhysicalSize::new(1280, 800);

    // Login.
    let inst = def.create().unwrap();
    inst.show().unwrap();
    set(&inst, "login-server", s("https://music.example.com"));
    set(&inst, "login-error", s("API Token 已失效，请重新输入"));
    set(&inst, "login-media-token-saved", Value::Bool(true));
    render(&window, size, &format!("{out}/login.png"));
    inst.hide().unwrap();

    // Main views.
    let inst = def.create().unwrap();
    inst.show().unwrap();
    set(&inst, "logged-in", Value::Bool(true));
    set(&inst, "server-label", s("https://music.example.com"));
    set(&inst, "view-title", s("专辑"));
    set(&inst, "view-loaded", Value::Number(500.0));
    set(&inst, "view-total", Value::Number(1234.0));
    set(&inst, "toolbar", toolbar_with(true));
    set(&inst, "grid", Value::Bool(true));
    // Columns for 1280 - 200 sidebar - 320 lyrics = 760px.
    set(&inst, "rows", cards(3));
    set(&inst, "has-track", Value::Bool(true));
    set(&inst, "playing-id", Value::Number(101.0));
    set(&inst, "np-title", s("夜に駆ける"));
    set(&inst, "np-artist", s("YOASOBI"));
    set(&inst, "np-quality", s("FLAC / 24bit / 48kHz"));
    set(&inst, "np-cover", Value::Image(cover(3)));
    set(&inst, "position-ms", Value::Number(83_000.0));
    set(&inst, "duration-ms", Value::Number(261_000.0));
    set(&inst, "position-text", s("1:23"));
    set(&inst, "duration-text", s("4:21"));
    set(&inst, "paused", Value::Bool(false));
    set(&inst, "volume", Value::Number(70.0));
    set(&inst, "lyrics", lyrics());
    set(&inst, "lyrics-synced", Value::Bool(true));
    set(&inst, "lyrics-active", Value::Number(2.0));
    set(&inst, "lyrics-source", s("LRCLIB"));
    render(&window, size, &format!("{out}/albums.png"));

    set(&inst, "grid", Value::Bool(false));
    set(&inst, "shuffle-all-visible", Value::Bool(true));
    set(&inst, "lines", lines());
    set(&inst, "view-title", s("曲目"));
    set(&inst, "view-loaded", Value::Number(10000.0));
    set(&inst, "view-total", Value::Number(10000.0));
    set(&inst, "notice", s("播放失败：loading failed"));
    render(&window, size, &format!("{out}/tracks.png"));

    // Album detail with a header cover.
    set(&inst, "notice", s(""));
    set(&inst, "shuffle-all-visible", Value::Bool(false));
    set(&inst, "view-title", s("君の名は。"));
    set(&inst, "view-subtitle", s("RADWIMPS"));
    set(
        &inst,
        "view-details",
        s("2016 · soundtrack · FLAC / 24bit / 48kHz · EMI Records"),
    );
    set(&inst, "has-view-cover", Value::Bool(true));
    set(&inst, "view-cover", Value::Image(cover(3)));
    set(&inst, "view-loaded", Value::Number(28.0));
    set(&inst, "view-total", Value::Number(28.0));
    set(&inst, "can-play", Value::Bool(true));
    set(&inst, "toolbar", toolbar_with(false));
    render(&window, size, &format!("{out}/album.png"));

    // Playlist detail with the editor bar.
    set(&inst, "view-title", s("夜晚歌单"));
    set(&inst, "view-subtitle", s(""));
    set(&inst, "view-details", s("收藏的慢歌 · 42 首 · 3:12:44"));
    set(&inst, "editable", Value::Bool(true));
    set(&inst, "editing-playlist", Value::Number(5.0));
    set(
        &inst,
        "playlist-names",
        model(["夜晚歌单", "通勤", "练琴"].into_iter().map(s).collect()),
    );
    render(&window, size, &format!("{out}/playlist.png"));
    set(&inst, "editable", Value::Bool(false));
    set(&inst, "editing-playlist", Value::Number(0.0));
    set(&inst, "has-view-cover", Value::Bool(false));
    set(&inst, "can-play", Value::Bool(false));

    set(&inst, "queue", queue());
    set(&inst, "queue-current", Value::Number(1.0));
    set(&inst, "show-queue", Value::Bool(true));
    set(&inst, "shuffle", Value::Bool(true));
    set(&inst, "repeat", Value::Number(2.0));
    set(&inst, "sleep-active", Value::Bool(true));
    set(&inst, "sleep-label", s("29:41"));
    render(&window, size, &format!("{out}/queue.png"));
    set(&inst, "show-queue", Value::Bool(false));

    // Search view.
    set(&inst, "view-title", s("搜索"));
    set(&inst, "view-subtitle", s("“夜”"));
    set(&inst, "view-details", s(""));
    set(&inst, "search-text", s("夜"));
    render(&window, size, &format!("{out}/search.png"));

    set(&inst, "show-settings", Value::Bool(true));
    set(&inst, "notice", s(""));
    let devices: Vec<Value> = [
        "系统默认",
        "扬声器 (Realtek(R) Audio)",
        "Topping E30 II",
        "wasapi/{1234}（未连接）",
    ]
    .into_iter()
    .map(s)
    .collect();
    set(
        &inst,
        "audio-devices",
        Value::Model(ModelRc::new(VecModel::from(devices))),
    );
    set(&inst, "audio-device-index", Value::Number(2.0));
    set(&inst, "exclusive", Value::Bool(true));
    set(&inst, "tray-available", Value::Bool(true));
    render(&window, size, &format!("{out}/settings.png"));

    // New playlist dialog.
    set(&inst, "show-settings", Value::Bool(false));
    set(&inst, "show-create-playlist", Value::Bool(true));
    set(&inst, "new-playlist-name", s("新歌单"));
    render(&window, size, &format!("{out}/create-playlist.png"));
}
