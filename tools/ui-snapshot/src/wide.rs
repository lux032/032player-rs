//! Wide-screen snapshots for the v3 Editorial redesign (`docs/UI-REDESIGN.md`):
//! the home, an album, the album list and the full-screen player at
//! 2300x1190, 1600x900 and 1280x800 (scale 1), with data shaped like the
//! final mockups (`prototypes/claude-v3-final/`). Long titles, works with
//! posters and multi-work OP / ED tags are covered too.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use slint::platform::software_renderer::MinimalSoftwareWindow;
use slint::{ComponentHandle, Image, PhysicalSize};
use slint_interpreter::{ComponentDefinition, ComponentInstance, Value};

use super::{art, bools, create, model, n, poster, render, s, set, set_g, st, strings};

pub const SIZES: [(u32, u32); 3] = [(2300, 1190), (1600, 900), (1280, 800)];

const ALBUMS: [(&str, &str, &str, &str); 24] = [
    ("pure", "PURE", "キタニタツヤ", "2026"),
    ("dekai", "DEKAI", "キタニタツヤ", "2026"),
    ("bluecheese", "ブルーチーズ", "Various Artists", "2026"),
    ("wakemeup", "Wake Me Up", "ReoNa", "2026"),
    ("fairyness", "Fairyness", "発熱巫女～ず", "2019"),
    ("billow", "Billow", "須田景凪", "2021"),
    ("quote", "Quote", "須田景凪", "2018"),
    ("ghostpop", "Ghost Pop", "須田景凪", "2023"),
    ("emoria", "Emoria", "安月名莉子", "2026"),
    ("shikanoko", "シカノコ", "久保ユリカ", "2026"),
    ("actezero", "Acte Zéro", "スタァライト九九組", "2025"),
    (
        "kamenrider50",
        "仮面ライダー 50th Anniversary SONG BEST BOX",
        "Various Artists",
        "2022",
    ),
    (
        "crossing6",
        "CrosSing Collection Vol.6",
        "Various Artists",
        "2026",
    ),
    ("photonaway", "A Photon Away", "nonoc", "2026"),
    ("amenohi", "雨の日", "wagataki", "2026"),
    ("lovehate", "LOVE HATE", "須田景凪", "2026"),
    (
        "lovesmileblood",
        "Love, Smile & Blood",
        "発熱巫女～ず",
        "2017",
    ),
    ("historybegins", "HIStory Begins", "発熱巫女～ず", "2015"),
    ("girlsmirror", "Girls in the Mirror", "発熱巫女～ず", "2016"),
    ("mimic", "mimic", "Various Artists", "2024"),
    ("kanjaa", "患者A", "Ado", "2025"),
    (
        "kimitoakashita",
        "君と明かした夜",
        "Various Artists",
        "2024",
    ),
    ("gunjou", "群青讃歌 / 遊生夢死", "Eve", "2023"),
    ("elza2", "ELZA2", "Various Artists", "2025"),
];

const LONG_ALBUM: &str = "BanG Dream! It's MyGO!!!!! ORIGINAL SOUNDTRACK";
const LONG_HERO: &str = "仮面ライダー 50th Anniversary SONG BEST BOX";

fn card(i: usize, added: &str) -> Value {
    let (c, t, a, y) = ALBUMS[i % ALBUMS.len()];
    st(&[
        ("id", n(i as f64 + 1.0)),
        ("title", s(t)),
        ("artist", s(a)),
        ("year", s(if added.is_empty() { y } else { "" })),
        ("artist-id", n(5.0)),
        ("favorite", Value::Bool(i % 5 == 1)),
        ("badge", s("")),
        ("note", s("")),
        ("added", s(added)),
        ("playing", Value::Bool(i == 1)),
        ("disc-count", n(if i.is_multiple_of(4) { 2.0 } else { 1.0 })),
        ("cover", Value::Image(art(c))),
    ])
}

/// Cards chunked into rows of `cols`, at most `max` (0: all).
fn rows(cards: &[Value], cols: usize, max: usize) -> Value {
    let cards = if max > 0 && cards.len() > max {
        &cards[..max]
    } else {
        cards
    };
    model(
        cards
            .chunks(cols.max(1))
            .map(|c| st(&[("items", model(c.to_vec()))]))
            .collect(),
    )
}

fn album_cards(count: usize) -> Vec<Value> {
    (0..count).map(|i| card(i, "")).collect()
}

fn added_cards() -> Vec<Value> {
    let when = [
        "入库于 2 天前",
        "入库于 2 天前",
        "入库于 3 天前",
        "入库于 3 天前",
    ];
    (0..10).map(|i| card(i, when[i.min(3)])).collect()
}

fn work_cards() -> Vec<Value> {
    [
        (
            "wakemeup",
            "ガンゲイル・オンライン II",
            "动画 · 2024",
            "OP 1 · ED 1",
            "收录 7 首",
        ),
        (
            "shikanoko",
            "しかのこのこのここしたんたん",
            "动画 · 2024",
            "OP 1",
            "收录 4 首",
        ),
        (
            "actezero",
            "少女☆歌劇 レヴュースタァライト",
            "动画 · 2018",
            "OP 2 · ED 3 · 插曲 14",
            "收录 10 首",
        ),
        (
            "lovesmileblood",
            "东方 Project",
            "游戏 · 1996",
            "改编 312",
            "收录 4 首",
        ),
        (
            "kamenrider50",
            "仮面ライダー THE MOVIE",
            "电影 · 2022",
            "主题曲 6",
            "收录 4 首",
        ),
        (
            "gunjou",
            "呪術廻戦 懐玉・玉折",
            "动画 · 2023",
            "OP 1",
            "收录 2 首",
        ),
    ]
    .iter()
    .enumerate()
    .map(|(i, (c, t, kind, uses, count))| {
        st(&[
            ("id", n(i as f64 + 31.0)),
            ("title", s(t)),
            ("artist", s(kind)),
            ("year", s("")),
            ("artist-id", n(0.0)),
            ("favorite", Value::Bool(false)),
            ("badge", s(if i == 2 { "少女☆歌劇" } else { "" })),
            ("note", s(uses)),
            ("added", s(count)),
            ("playing", Value::Bool(i == 0)),
            ("cover", Value::Image(poster(c))),
        ])
    })
    .collect()
}

struct RowData<'a> {
    id: f64,
    cover: Image,
    title: &'a str,
    subtitle: &'a str,
    note: &'a str,
    duration: &'a str,
}

fn home_row(i: usize, r: RowData<'_>, extra: &[(&str, Value)]) -> Value {
    let mut f = vec![
        ("album-id", n(3.0)),
        ("artist-id", n(5.0)),
        ("favorite", Value::Bool(false)),
        ("tag", s("")),
        ("tag-accent", Value::Bool(false)),
        ("link", s("")),
        ("link-id", n(0.0)),
        ("note", s(r.note)),
        ("dimmed", Value::Bool(false)),
        ("index", n(i as f64)),
        ("id", n(r.id)),
        ("title", s(r.title)),
        ("subtitle", s(r.subtitle)),
        ("cover", Value::Image(r.cover)),
        ("percent", n(-1.0)),
        ("duration", s(r.duration)),
        ("covers", model(Vec::new())),
    ];
    f.extend(extra.iter().cloned());
    st(&f)
}

fn mixes() -> Value {
    let data = [
        (
            ["wakemeup", "bluecheese", "crossing6", "mimic"],
            "都市霓虹的放克派对",
            "City Pop / 放克 · 25 首 · 1 小时 32 分",
        ),
        (
            ["shikanoko", "emoria", "actezero", "kamenrider50"],
            "元气满满的欢快时光",
            "元气欢快 · 25 首 · 1 小时 33 分",
        ),
        (
            ["lovehate", "dekai", "kanjaa", "gunjou"],
            "壮阔史诗的摇滚交响",
            "壮阔史诗 · 摇滚 · 25 首 · 1 小时 49 分",
        ),
        (
            ["photonaway", "amenohi", "billow", "elza2"],
            "梦幻空灵的后摇空间",
            "梦幻空灵 · 后摇 / 实验 · 25 首 · 1 小时 18 分",
        ),
        (
            ["quote", "ghostpop", "billow", "lovehate"],
            "从 n-buna 出发",
            "创作者 · 13 首 · 54 分钟",
        ),
    ];
    model(
        data.iter()
            .enumerate()
            .map(|(i, (covers, t, sub))| {
                let tiles = covers.iter().map(|c| Value::Image(art(c))).collect();
                home_row(
                    i,
                    RowData {
                        id: 500.0 + i as f64,
                        cover: Image::default(),
                        title: t,
                        subtitle: sub,
                        note: "",
                        duration: "",
                    },
                    &[("covers", model(tiles))],
                )
            })
            .collect(),
    )
}

fn recent() -> Value {
    let data = [
        (
            "dekai",
            "F.A.C.E.",
            "キタニタツヤ · DEKAI",
            "正在播放",
            "2:29",
        ),
        (
            "mimic",
            "Go-Getters",
            "Mori Calliope · PHANTOMIME",
            "今天 23:37",
            "3:12",
        ),
        (
            "mimic",
            "SNEAKING revenge",
            "Mori Calliope, Lotus Juice · PHANTOMIME",
            "今天 23:33",
            "3:41",
        ),
        (
            "wakemeup",
            "Wake Me Up",
            "ReoNa · Wake Me Up",
            "今天 22:10",
            "4:02",
        ),
        (
            "amenohi",
            "雨の日",
            "wagataki · 雨の日",
            "昨天 21:48",
            "3:55",
        ),
    ];
    model(
        data.iter()
            .enumerate()
            .map(|(i, (c, t, sub, when, d))| {
                home_row(
                    i,
                    RowData {
                        id: if i == 0 { 101.0 } else { 600.0 + i as f64 },
                        cover: art(c),
                        title: t,
                        subtitle: sub,
                        note: when,
                        duration: d,
                    },
                    &[],
                )
            })
            .collect(),
    )
}

fn resume() -> Value {
    let data = [
        (
            "billow",
            "Billow",
            "須田景凪 · 第 6 首",
            46.0,
            "1:52 / 4:03",
        ),
        (
            "actezero",
            "Acte Zéro",
            "スタァライト九九組 · 第 3 首",
            72.0,
            "3:10 / 4:24",
        ),
        (
            "crossing6",
            "CrosSing Collection Vol.6",
            "Various Artists · 第 9 首",
            18.0,
            "0:41 / 3:48",
        ),
        (
            "photonaway",
            "A Photon Away",
            "nonoc · 第 2 首",
            60.0,
            "2:35 / 4:18",
        ),
    ];
    model(
        data.iter()
            .enumerate()
            .map(|(i, (c, t, sub, p, d))| {
                home_row(
                    i,
                    RowData {
                        id: 700.0 + i as f64,
                        cover: art(c),
                        title: t,
                        subtitle: sub,
                        note: "昨天 21:48",
                        duration: d,
                    },
                    &[("percent", n(*p))],
                )
            })
            .collect(),
    )
}

fn songs() -> Value {
    let data = [
        (
            "wakemeup",
            "Wake Me Up",
            "ReoNa",
            "OP",
            "ガンゲイル・オンライン II",
            "4:02",
        ),
        (
            "dekai",
            "青のすみか",
            "キタニタツヤ",
            "OP",
            "呪術廻戦 懐玉・玉折",
            "3:17",
        ),
        (
            "shikanoko",
            "シカ色デイズ",
            "シカ部",
            "OP",
            "しかのこのこのここしたんたん",
            "3:28",
        ),
        (
            "dekai",
            "ファタール",
            "GEMN",
            "OP",
            "【推しの子】 第 2 期",
            "3:39",
        ),
        (
            "actezero",
            "Fly Me to the Star",
            "スタァライト九九組",
            "ED",
            "少女☆歌劇 レヴュースタァライト",
            "4:36",
        ),
        (
            "kamenrider50",
            "Let's try together",
            "大久保薫",
            "主题曲",
            "仮面ライダー THE MOVIE",
            "3:51",
        ),
    ];
    model(
        data.iter()
            .enumerate()
            .map(|(i, (c, t, a, role, work, d))| {
                home_row(
                    i,
                    RowData {
                        id: 800.0 + i as f64,
                        cover: art(c),
                        title: t,
                        subtitle: a,
                        note: "",
                        duration: d,
                    },
                    &[
                        ("tag", s(role)),
                        ("tag-accent", Value::Bool(*role == "OP" || *role == "ED")),
                        ("link", s(work)),
                        ("link-id", n(31.0 + i as f64)),
                    ],
                )
            })
            .collect(),
    )
}

/// The fields of a track row.
fn track(
    i: usize,
    number: usize,
    title: &str,
    artist: &str,
    duration: &str,
    fav: bool,
) -> Vec<(&'static str, Value)> {
    vec![
        ("id", n(if i == 0 { 101.0 } else { 900.0 + i as f64 })),
        ("index", n(i as f64)),
        ("title", s(title)),
        ("artist", s(artist)),
        ("album", s("DEKAI")),
        ("duration", s(duration)),
        ("favorite", Value::Bool(fav)),
        ("number", s(&number.to_string())),
        ("album-id", n(3.0)),
        ("artist-id", n(5.0)),
        ("tag", s("")),
        ("tag-accent", Value::Bool(false)),
        ("note", s("")),
        ("disabled", Value::Bool(false)),
        ("blocked", Value::Bool(false)),
        ("credits", model(Vec::new())),
        ("tieup", s("")),
        ("tieup-count", n(0.0)),
        ("tieup-tooltip", s("")),
        ("tieup-works", model(Vec::new())),
    ]
}

fn credits() -> Value {
    model(
        [
            ("作词 ", 0.0),
            ("キタニタツヤ", 5.0),
            (" · ", 0.0),
            ("作曲 ", 0.0),
            ("キタニタツヤ", 5.0),
            (" · ", 0.0),
            ("编曲 ", 0.0),
            ("キタニタツヤ", 5.0),
        ]
        .iter()
        .map(|(t, id)| st(&[("text", s(t)), ("id", n(*id))]))
        .collect(),
    )
}

fn tieup_work(id: f64, title: &str, translated: &str, meta: &str, cover: &str) -> Value {
    st(&[
        ("id", n(id)),
        ("title", s(title)),
        ("translated", s(translated)),
        ("meta", s(meta)),
        ("usage", s("OP")),
        ("poster", Value::Image(poster(cover))),
    ])
}

fn album_tracks() -> Value {
    let data = [
        ("F.A.C.E.", "キタニタツヤ", "2:29", true),
        ("火種", "キタニタツヤ", "3:24", false),
        ("ファタール", "キタニタツヤ, GEMN", "3:39", true),
        ("めんどくせいな", "キタニタツヤ", "2:52", false),
        ("カルチャー", "キタニタツヤ", "2:57", false),
        ("なくしもの", "キタニタツヤ", "4:09", false),
        ("あなたのことをおしえて", "キタニタツヤ", "2:30", false),
        ("たったひとつの", "キタニタツヤ", "4:40", false),
        ("まなざしは光", "キタニタツヤ", "3:25", true),
        ("スカー", "キタニタツヤ", "3:51", false),
        ("青のすみか", "キタニタツヤ", "3:17", true),
    ];
    let oshi = tieup_work(
        41.0,
        "【推しの子】 第 2 期",
        "Oshi no Ko Season 2",
        "动画 · 2024",
        "kimitoakashita",
    );
    let jjk = tieup_work(
        42.0,
        "呪術廻戦 懐玉・玉折",
        "Jujutsu Kaisen: Hidden Inventory",
        "动画 · 2023",
        "gunjou",
    );
    model(
        data.iter()
            .enumerate()
            .map(|(i, (t, a, d, fav))| {
                let mut f = track(i, i + 1, t, a, d, *fav);
                if matches!(i, 0 | 2 | 10) {
                    f.push(("credits", credits()));
                }
                if i == 2 {
                    f.push(("tieup", s("OP")));
                    f.push(("tieup-count", n(1.0)));
                    f.push(("tieup-works", model(vec![oshi.clone()])));
                }
                if i == 10 {
                    f.push(("tieup", s("OP")));
                    f.push(("tieup-count", n(2.0)));
                    f.push(("tieup-works", model(vec![jjk.clone(), oshi.clone()])));
                }
                st(&f)
            })
            .collect(),
    )
}

fn album_works() -> Value {
    let uses = |items: &[(&str, &str)]| {
        model(
            items
                .iter()
                .map(|(l, t)| st(&[("label", s(l)), ("title", s(t))]))
                .collect(),
        )
    };
    model(vec![
        st(&[
            ("id", n(42.0)),
            ("title", s("呪術廻戦 懐玉・玉折")),
            ("translated", s("Jujutsu Kaisen: Hidden Inventory")),
            ("meta", s("动画 · 2023")),
            ("relation", s("OP")),
            ("usage", s("OP")),
            ("ost", Value::Bool(false)),
            ("poster", Value::Image(poster("gunjou"))),
            ("chip", Value::Image(poster("gunjou"))),
            ("uses", uses(&[("OP", "青のすみか")])),
        ]),
        st(&[
            ("id", n(41.0)),
            ("title", s("【推しの子】 第 2 期")),
            ("translated", s("Oshi no Ko Season 2")),
            ("meta", s("动画 · 2024")),
            ("relation", s("OP")),
            ("usage", s("OP")),
            ("ost", Value::Bool(false)),
            ("poster", Value::Image(poster("kimitoakashita"))),
            ("chip", Value::Image(poster("kimitoakashita"))),
            ("uses", uses(&[("OP", "ファタール")])),
        ]),
    ])
}

fn facts(items: &[(&str, &str)]) -> Value {
    model(
        items
            .iter()
            .map(|(l, v)| st(&[("label", s(l)), ("value", s(v))]))
            .collect(),
    )
}

fn detail(title: &str) -> Value {
    st(&[
        ("kicker", s("")),
        ("kind", super::ev("DetailKind", "album")),
        ("id", n(3.0)),
        ("title", s(title)),
        ("subtitle", s("キタニタツヤ")),
        ("subtitle-id", n(5.0)),
        ("meta", s("2026 · 11 首 · 38 分 42 秒 · J-Rock")),
        ("quality", s("FLAC 24bit / 48kHz")),
        ("description", s("")),
        ("favorite", Value::Bool(true)),
        ("custom-artwork", Value::Bool(false)),
    ])
}

fn queue() -> Value {
    let manual = [
        ("F.A.C.E.", "キタニタツヤ", "2:29", "dekai"),
        ("火種", "キタニタツヤ", "3:24", "dekai"),
        ("ファタール", "GEMN", "3:39", "dekai"),
    ];
    let auto = [
        ("星座になれたら", "結束バンド", "4:18", "pure", "听感相似"),
        ("青のすみか", "キタニタツヤ", "3:17", "gunjou", "同作品"),
        ("Wake Me Up", "ReoNa", "4:02", "wakemeup", "共现歌单"),
        ("Ghost Pop", "須田景凪", "3:36", "ghostpop", "同作者"),
        ("A Photon Away", "nonoc", "4:18", "photonaway", "听感相似"),
    ];
    let mut rows = Vec::new();
    for (i, (t, a, d, c)) in manual.iter().enumerate() {
        rows.push(st(&[
            ("kind", n(0.0)),
            ("uid", n(i as f64 + 1.0)),
            ("play-index", n(i as f64)),
            ("number", n(i as f64 + 1.0)),
            ("title", s(t)),
            ("artist", s(a)),
            ("duration", s(d)),
            ("cover", Value::Image(art(c))),
            ("is-auto", Value::Bool(false)),
            ("reason", s("")),
            ("album-id", n(3.0)),
            ("has-artists", Value::Bool(true)),
        ]));
    }
    rows.push(st(&[
        ("kind", n(1.0)),
        ("uid", n(-1.0)),
        ("play-index", n(manual.len() as f64)),
        ("number", n(0.0)),
        ("title", s("")),
        ("artist", s("")),
        ("duration", s("")),
        ("cover", Value::Image(Image::default())),
        ("is-auto", Value::Bool(false)),
        ("reason", s("")),
        ("album-id", n(0.0)),
        ("has-artists", Value::Bool(false)),
    ]));
    for (j, (t, a, d, c, reason)) in auto.iter().enumerate() {
        let idx = manual.len() + j;
        rows.push(st(&[
            ("kind", n(0.0)),
            ("uid", n(idx as f64 + 1.0)),
            ("play-index", n(idx as f64)),
            ("number", n(idx as f64 + 1.0)),
            ("title", s(t)),
            ("artist", s(a)),
            ("duration", s(d)),
            ("cover", Value::Image(art(c))),
            ("is-auto", Value::Bool(true)),
            ("reason", s(reason)),
            ("album-id", n(4.0)),
            ("has-artists", Value::Bool(true)),
        ]));
    }
    rows.push(st(&[
        ("kind", n(2.0)),
        ("uid", n(-2.0)),
        ("play-index", n((manual.len() + auto.len() - 1) as f64)),
        ("number", n(0.0)),
        ("title", s("")),
        ("artist", s("")),
        ("duration", s("")),
        ("cover", Value::Image(Image::default())),
        ("is-auto", Value::Bool(false)),
        ("reason", s("")),
        ("album-id", n(0.0)),
        ("has-artists", Value::Bool(false)),
    ]));
    model(rows)
}

fn lyrics() -> Value {
    let lines = [
        ("またこうなるのね", "又是这样吗"),
        ("人間には呆れてしまいました", "我对人类感到厌倦了"),
        (
            "「愛に生きるか…」の逆になろう",
            "做成\u{201c}为爱而活……\u{201d}的反面吧",
        ),
        ("「敵対」をただやっていく", "只是去进行\u{201c}对抗\u{201d}"),
        ("コミュニケーションの時代は終わりさ", "沟通的时代已经结束了"),
        ("顔のない言葉が飛び交う街で", "在无脸之言漫天飞舞的街上"),
        ("誰も彼も同じ顔をしている", "每个人都长着同样的脸"),
        ("それでも僕は僕でいたい", "即便如此我仍想做我自己"),
    ];
    model(
        lines
            .iter()
            .map(|(t, tr)| st(&[("text", s(t)), ("translation", s(tr))]))
            .collect(),
    )
}

fn pane_tracks() -> Value {
    let data = [
        ("ずうっといっしょ！", "キタニタツヤ", "3:02"),
        ("いらないもの", "キタニタツヤ, なとり", "3:05"),
        ("かすかなはな", "キタニタツヤ, BABYMETAL", "3:06"),
        ("WITCHCRAFT", "キタニタツヤ", "2:12"),
        ("PURE", "キタニタツヤ", "3:40"),
        ("Moonthief", "キタニタツヤ", "3:30"),
    ];
    model(
        data.iter()
            .enumerate()
            .map(|(i, (t, a, d))| st(&track(i + 20, i + 1, t, a, d, false)))
            .collect(),
    )
}

fn pane(title: &str) -> Value {
    st(&[
        ("tab", n(0.0)),
        ("list", super::ev("TrackListId", "spotlight-recent")),
        ("album-id", n(3.0)),
        ("title", s(title)),
        ("artist", s("キタニタツヤ")),
        ("artist-id", n(5.0)),
        ("facts", s("2026 · 11 首 · 36:55")),
        ("spec", s("FLAC 24bit / 48kHz")),
        ("note", s("入库于 2 天前")),
        ("resume", s("")),
        ("more", s("查看全部 11 首")),
        ("favorite", Value::Bool(false)),
        ("cover", Value::Image(art("pure"))),
        ("track-count", n(11.0)),
    ])
}

/// Card layouts reported by the UI: grid -> (columns, most cards).
type Layouts = Rc<RefCell<HashMap<String, (usize, usize)>>>;

fn grid_arg(args: &[Value], i: usize) -> String {
    match args.get(i) {
        Some(Value::EnumerationValue(_, v)) => v.clone(),
        _ => String::new(),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn int_arg(args: &[Value], i: usize) -> i32 {
    match args.get(i) {
        Some(Value::Number(v)) => *v as i32,
        _ => 0,
    }
}

/// Renders (and throws away) a few frames: `changed` handlers only run once
/// the properties are evaluated by a real render.
fn settle(window: &MinimalSoftwareWindow, size: PhysicalSize) {
    window.set_size(size);
    let (w, h) = (size.width as usize, size.height as usize);
    let mut buf = vec![super::Px::default(); w * h];
    for _ in 0..2 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut buf, w);
        });
    }
}

/// Fills the grids that follow the layout (albums, home 最近添加).
fn fill_grids(inst: &ComponentInstance, layouts: &Layouts) {
    let l = layouts.borrow();
    if std::env::var_os("UI_SNAPSHOT_DEBUG").is_some() {
        eprintln!("layouts {:?}", *l);
    }
    let (cols, _) = l.get("albums").copied().unwrap_or((6, 0));
    set(inst, "album-rows", rows(&album_cards(24), cols, 0));
    let (cols, max) = l.get("home-albums").copied().unwrap_or((4, 8));
    let max = if max == 0 { cols * 2 } else { max };
    set_g(
        inst,
        "Home",
        "recent-albums",
        rows(&added_cards(), cols, max),
    );
}

fn shot(
    inst: &ComponentInstance,
    window: &MinimalSoftwareWindow,
    layouts: &Layouts,
    size: PhysicalSize,
    path: &str,
) {
    // `changed` callbacks (window width -> `Shell` -> grid columns) report
    // the layout a few frames late.
    for _ in 0..2 {
        settle(window, size);
        fill_grids(inst, layouts);
    }
    settle(window, size);
    render(window, size, path);
}

fn shell(inst: &ComponentInstance) {
    set(inst, "logged-in", Value::Bool(true));
    set(inst, "server-label", s("https://music.032.example"));
    set(inst, "server-status", s("music.032 · 18,930 首"));
    set(inst, "search-shortcut", s("Ctrl+K"));
    set(inst, "browse", Value::Bool(true));
    set(inst, "has-home", Value::Bool(true));
    set(
        inst,
        "album-sorts",
        strings(&[
            "名称",
            "最近添加",
            "最近播放",
            "发行日期",
            "发行年份",
            "随机",
        ]),
    );
    set(
        inst,
        "album-sort-desc",
        bools(&[false, true, true, true, true, false]),
    );
    let q = st(&[
        ("sort", n(1.0)),
        ("desc", Value::Bool(true)),
        ("search", s("")),
        ("year-from", s("2018")),
        ("year-to", s("2026")),
    ]);
    set(inst, "q-albums", q);
    set(inst, "album-filter", s("艺人：須田景凪"));
    set(inst, "album-loaded", n(1286.0));
    set(inst, "album-total", n(1286.0));
    set(
        inst,
        "side-playlists",
        model(
            [
                (
                    "elza2",
                    "HOSHIMACHI SUISEI ARENA TOUR 2025",
                    "20 首 · 1 小时 14 分钟",
                ),
                ("fairyness", "音乃瀬奏", "1 首 · 3 分钟"),
                ("ghostpop", "通勤", "86 首 · 5 小时 2 分钟"),
            ]
            .iter()
            .enumerate()
            .map(|(i, (c, t, m))| {
                st(&[
                    ("id", n(i as f64 + 1.0)),
                    ("title", s(t)),
                    ("meta", s(m)),
                    ("year", s("")),
                    ("cover", Value::Image(art(c))),
                ])
            })
            .collect(),
        ),
    );
    set(inst, "has-track", Value::Bool(true));
    set(inst, "playing-id", n(101.0));
    set(inst, "np-title", s("F.A.C.E."));
    set(inst, "np-artist", s("キタニタツヤ"));
    set(inst, "np-album", s("DEKAI"));
    set(inst, "np-year", s("2026"));
    set(inst, "np-quality", s("FLAC 24bit / 48kHz"));
    set(inst, "np-favorite", Value::Bool(true));
    set(inst, "np-cover", Value::Image(art("dekai")));
    set(inst, "np-cover-large", Value::Image(art("dekai")));
    set(inst, "full-cover", Value::Image(art("dekai")));
    set(inst, "exclusive", Value::Bool(true));
    set(inst, "exclusive-mode", Value::Number(1.0));
    set(inst, "preferred-exclusive-mode", Value::Number(1.0));
    set(inst, "asio-configured", Value::Bool(false));
    set(inst, "sleep-active", Value::Bool(true));
    set(inst, "sleep-label", s("29:41"));
    set(inst, "repeat", n(2.0));
    set(inst, "position-ms", n(7_000.0));
    set(inst, "duration-ms", n(149_000.0));
    set(inst, "position-text", s("0:07"));
    set(inst, "duration-text", s("2:29"));
    set(inst, "paused", Value::Bool(false));
    set(inst, "volume", n(68.0));
    set(inst, "queue", queue());
    set(inst, "queue-total", n(8.0));
    set(inst, "queue-current", n(0.0));
    set(inst, "queue-position", n(0.0));
    set_g(inst, "RadioState", "supported", Value::Bool(true));
    set_g(inst, "RadioState", "auto-play", Value::Bool(true));
    set_g(inst, "RadioState", "active-radio", Value::Bool(true));
    set_g(inst, "RadioState", "seed-title", s("F.A.C.E."));
    set_g(
        inst,
        "RadioState",
        "section-subtitle",
        s("基于《F.A.C.E.》 · 声学 + 元数据混合"),
    );
    set_g(
        inst,
        "RadioState",
        "tail-title",
        s("队列末尾将自动续播相似曲目"),
    );
    set_g(
        inst,
        "RadioState",
        "tail-subtitle",
        s("剩余不足 2 首时自动静默补充下一批"),
    );
    set_g(inst, "RadioState", "tail-action-label", s("再加一批"));
    set_g(inst, "RadioState", "tail-action-kind", s("load-more"));
    set(inst, "lyrics", lyrics());
    set(inst, "lyrics-synced", Value::Bool(true));
    set(inst, "lyrics-active", n(2.0));
    set(inst, "lyrics-source", s("LRC 文件"));
    set(inst, "lyrics-translated-lines", Value::Bool(true));
    set(inst, "lyrics-can-search", Value::Bool(true));
    set(inst, "lyrics-can-translate", Value::Bool(true));
    set(inst, "lyrics-translation-shown", Value::Bool(true));
    set(inst, "panel-open", Value::Bool(true));
}

fn home(inst: &ComponentInstance, title: &str) {
    set_g(inst, "Home", "loading", Value::Bool(false));
    set_g(
        inst,
        "Home",
        "tab-labels",
        strings(&["最新入库", "最近在听", "作品聚焦", "随机重温"]),
    );
    set_g(
        inst,
        "Home",
        "tab-ids",
        model(vec![n(0.0), n(1.0), n(2.0), n(3.0)]),
    );
    set_g(inst, "Home", "tab", n(0.0));
    set_g(inst, "Home", "pane", pane(title));
    set_g(inst, "Home", "pane-tracks", pane_tracks());
    set_g(inst, "Home", "mixes", mixes());
    set_g(inst, "Home", "recent", recent());
    set_g(inst, "Home", "resume", resume());
    set_g(inst, "Home", "works", rows(&work_cards(), 1000, 0));
    set_g(inst, "Home", "works-count", n(733.0));
    set_g(inst, "Home", "works-pages", n(4.0));
    set_g(inst, "Home", "songs", songs());
    set_g(inst, "Home", "songs-count", n(112.0));
    set_g(inst, "Home", "songs-pages", n(6.0));
    const BARS: [f64; 26] = [
        4.0, 6.0, 10.0, 14.0, 9.0, 18.0, 22.0, 16.0, 25.0, 30.0, 28.0, 36.0, 41.0, 33.0, 38.0,
        46.0, 52.0, 44.0, 58.0, 63.0, 55.0, 61.0, 70.0, 66.0, 74.0, 92.0,
    ];
    set_g(
        inst,
        "Home",
        "years",
        model(
            BARS.iter()
                .enumerate()
                .map(|(i, h)| {
                    st(&[
                        ("year", n(2001.0 + i as f64)),
                        ("count", n(*h)),
                        ("height", n((h * 100.0 / 92.0).round())),
                    ])
                })
                .collect(),
        ),
    );
    set_g(inst, "Home", "year", n(2026.0));
    set_g(inst, "Home", "year-total", n(92.0));
    set_g(inst, "Home", "chronicle", rows(&album_cards(10), 1000, 0));
    set_g(inst, "Home", "playlists", rows(&album_cards(6), 1000, 0));
    set_g(
        inst,
        "Home",
        "stats",
        st(&[
            ("albums", n(1286.0)),
            ("tracks", n(18930.0)),
            ("works", n(733.0)),
            ("last-scan", s("上次扫描 2026年11月6日 20:00")),
            ("pending", n(3.0)),
        ]),
    );
}

fn album(inst: &ComponentInstance) {
    set(inst, "detail", detail("DEKAI"));
    set(inst, "detail-cover", Value::Image(art("dekai")));
    set(inst, "detail-tracks", album_tracks());
    set(inst, "album-works", album_works());
    set(
        inst,
        "album-release",
        facts(&[
            ("类型", "专辑"),
            ("发行日期", "2026-04-23"),
            ("厂牌", "Sony Music Labels"),
            ("碟数", "1"),
            ("目录号", "SRCL-13001"),
        ]),
    );
    set(
        inst,
        "album-media",
        facts(&[("格式", "FLAC / 24bit / 48kHz"), ("容量", "412 MB")]),
    );
    set(
        inst,
        "album-people",
        model(
            [
                ("キタニタツヤ", "演唱 · 作曲 · 11 首"),
                ("GEMN", "演唱 · 1 首"),
            ]
            .iter()
            .enumerate()
            .map(|(i, (name, roles))| {
                st(&[
                    ("id", n(i as f64 + 5.0)),
                    ("name", s(name)),
                    ("roles", s(roles)),
                    (
                        "avatar",
                        Value::Image(art(if i == 0 { "pure" } else { "dekai" })),
                    ),
                ])
            })
            .collect(),
        ),
    );
    set(inst, "can-go-back", Value::Bool(true));
}

pub fn run(def: &ComponentDefinition, window: &MinimalSoftwareWindow, out: &str) {
    let inst = create(def);
    inst.show().unwrap();
    let layouts: Layouts = Rc::default();
    let l = layouts.clone();
    inst.set_callback("card-layout-changed", move |args| {
        let grid = grid_arg(args, 0);
        let cols = usize::try_from(int_arg(args, 1).max(1)).unwrap_or(1);
        l.borrow_mut().entry(grid).or_insert((cols, 0)).0 = cols;
        Value::Void
    })
    .unwrap();
    let l = layouts.clone();
    inst.set_callback("card-limit-changed", move |args| {
        let grid = grid_arg(args, 0);
        let max = usize::try_from(int_arg(args, 1).max(0)).unwrap_or(0);
        l.borrow_mut().entry(grid).or_insert((1, max)).1 = max;
        Value::Void
    })
    .unwrap();
    shell(&inst);
    home(&inst, "PURE");
    album(&inst);

    // Side-by-side fake playback states for corner badge regression checks.
    if std::env::var("UI_SNAPSHOT_ONLY").as_deref() == Ok("badges") {
        set(&inst, "page", n(0.0));
        for (w, h, light) in [(1280, 800, false), (1600, 900, false), (1280, 800, true)] {
            set(&inst, "theme-mode", n(if light { 2.0 } else { 1.0 }));
            let size = PhysicalSize::new(w, h);
            for _ in 0..3 {
                settle(window, size);
                fill_grids(&inst, &layouts);
            }
            let cols = layouts.borrow().get("albums").copied().unwrap_or((4, 0)).0;
            let mut cards = album_cards(24);
            for (i, card) in cards.iter_mut().take(3).enumerate() {
                if let Value::Struct(card) = card {
                    card.set_field("playing".into(), Value::Bool(true));
                    if i == 2 {
                        card.set_field("badge".into(), s("合作"));
                    }
                }
            }
            set(&inst, "album-rows", rows(&cards, cols, 0));
            let prefix = if light { "light-" } else { "" };
            render(
                window,
                size,
                &format!("{out}/{prefix}album-playing-badges-{w}.png"),
            );
        }
        inst.hide().unwrap();
        return;
    }

    // Exercise both split-button hover regions with actual pointer events.
    if std::env::var("UI_SNAPSHOT_ONLY").as_deref() == Ok("play-hover") {
        let size = PhysicalSize::new(1280, 800);
        for (kind, name) in [("artist", "artist"), ("versions", "versions")] {
            set(&inst, "page", n(10.0));
            let mut info = match detail("須田景凪") {
                Value::Struct(info) => info,
                _ => unreachable!(),
            };
            info.set_field("kind".into(), super::ev("DetailKind", kind));
            set(&inst, "detail", Value::Struct(info));
            let base = format!("{out}/{name}-play-idle.png");
            shot(&inst, window, &layouts, size, &base);
            let pixels = image::open(&base).unwrap().to_rgba8();
            let mut longest = (0, 0, 0);
            for y in 50..360 {
                let mut start = 0;
                for x in 64..1200 {
                    let [r, g, b, _] = pixels.get_pixel(x, y).0;
                    if (141..=210).contains(&r) && (125..=210).contains(&g) && b == 255 {
                        if start == 0 {
                            start = x;
                        }
                        if x - start > longest.2 {
                            longest = (start, y, x - start);
                        }
                    } else {
                        start = 0;
                    }
                }
            }
            let (x, y, width) = longest;
            assert!(width > 120, "Split play button must be visible");
            for (state, px) in [("main-hover", x + 30), ("arrow-hover", x + width - 15)] {
                let position = slint::LogicalPosition::new(px as f32, y as f32 + 2.0);
                window.dispatch_event(slint::platform::WindowEvent::PointerMoved { position });
                let path = format!("{out}/{name}-play-{state}.png");
                render(window, size, &path);
                let frame = image::open(&path).unwrap();
                frame
                    .crop_imm(x.saturating_sub(16), y.saturating_sub(20), width + 32, 72)
                    .save(format!("{out}/{name}-play-{state}-detail.png"))
                    .unwrap();
            }
            if kind == "artist" {
                let position = slint::LogicalPosition::new((x + width - 15) as f32, y as f32 + 2.0);
                let button = slint::platform::PointerEventButton::Left;
                window.dispatch_event(slint::platform::WindowEvent::PointerPressed {
                    position,
                    button,
                });
                window.dispatch_event(slint::platform::WindowEvent::PointerReleased {
                    position,
                    button,
                });
                render(window, size, &format!("{out}/artist-play-menu.png"));
                window.dispatch_event(slint::platform::WindowEvent::KeyPressed {
                    text: slint::platform::Key::Escape.into(),
                });
                window.dispatch_event(slint::platform::WindowEvent::KeyReleased {
                    text: slint::platform::Key::Escape.into(),
                });
            }
            window.dispatch_event(slint::platform::WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(0.0, 0.0),
            });
        }
        inst.hide().unwrap();
        return;
    }

    // Tall poster and landscape cover adaptation across grid, detail, panel,
    // spotlight and full-screen player.
    if std::env::var("UI_SNAPSHOT_ONLY").as_deref() == Ok("posters") {
        let make_art = |w: u32, h: u32, c1: [u8; 3], c2: [u8; 3]| -> Image {
            let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
            for (idx, p) in buf.make_mut_slice().iter_mut().enumerate() {
                let x = (idx as u32) % w;
                let y = (idx as u32) / w;
                let border = x < 4 || y < 4 || x + 4 >= w || y + 4 >= h;
                if border {
                    *p = slint::Rgba8Pixel {
                        r: 235,
                        g: 100,
                        b: 140,
                        a: 255,
                    };
                } else {
                    let t = y as f32 / h.max(1) as f32;
                    let mix = |a: u8, b: u8| ((a as f32) * (1.0 - t) + (b as f32) * t) as u8;
                    *p = slint::Rgba8Pixel {
                        r: mix(c1[0], c2[0]),
                        g: mix(c1[1], c2[1]),
                        b: mix(c1[2], c2[2]),
                        a: 255,
                    };
                }
            }
            Image::from_rgba8(buf)
        };
        let tall = make_art(320, 480, [255, 220, 235], [170, 140, 230]);
        let wide_art = make_art(480, 270, [130, 200, 240], [60, 90, 180]);
        let square = make_art(360, 360, [250, 210, 110], [210, 90, 60]);

        let size = PhysicalSize::new(1600, 900);
        set(&inst, "theme-mode", n(2.0));
        set(&inst, "page", n(0.0));
        for _ in 0..3 {
            super::settle(window);
            window.set_size(size);
            fill_grids(&inst, &layouts);
        }
        let cols = layouts.borrow().get("albums").copied().unwrap_or((5, 0)).0;
        let mut cards = album_cards(10);
        for (idx, img) in [
            (0, square.clone()),
            (1, tall.clone()),
            (2, wide_art.clone()),
            (3, tall.clone()),
            (4, square.clone()),
            (5, tall.clone()),
            (6, wide_art.clone()),
        ] {
            if let Value::Struct(mut st_card) = cards[idx].clone() {
                st_card.set_field("cover".into(), Value::Image(img));
                if idx == 1 || idx == 2 {
                    st_card.set_field("disc-count".into(), n(2.0));
                }
                cards[idx] = Value::Struct(st_card);
            }
        }
        set(&inst, "album-rows", rows(&cards, cols, 0));
        set(&inst, "detail-cover", Value::Image(tall.clone()));
        set(&inst, "np-cover", Value::Image(tall.clone()));
        set(&inst, "np-cover-large", Value::Image(tall.clone()));
        set(&inst, "full-cover", Value::Image(tall.clone()));
        let mut p = pane("MURI MURI EVOLUTION");
        if let Value::Struct(mut st_pane) = p {
            st_pane.set_field("cover".into(), Value::Image(tall.clone()));
            p = Value::Struct(st_pane);
        }
        set_g(&inst, "Home", "pane", p);

        render(window, size, &format!("{out}/posters-albums-light.png"));
        set(&inst, "page", n(10.0));
        render(window, size, &format!("{out}/posters-detail-light.png"));
        set(&inst, "theme-mode", n(1.0));
        set(&inst, "page", n(20.0));
        render(window, size, &format!("{out}/posters-home-dark.png"));
        set(&inst, "full-open", Value::Bool(true));
        render(window, size, &format!("{out}/posters-player-dark.png"));
        inst.hide().unwrap();
        return;
    }

    // Header typography: short CJK, long Japanese, and long Latin titles.
    if std::env::var("UI_SNAPSHOT_ONLY").as_deref() == Ok("typography") {
        for (w, h) in SIZES {
            let size = PhysicalSize::new(w, h);
            for (name, title) in [
                ("short", "純粋心"),
                ("japanese", "風の中は走るっきゃないっ！"),
                ("latin", LONG_ALBUM),
            ] {
                set(&inst, "detail", detail(title));
                set(&inst, "page", n(10.0));
                shot(
                    &inst,
                    window,
                    &layouts,
                    size,
                    &format!("{out}/album-type-{name}-{w}.png"),
                );
                set_g(&inst, "Home", "pane", pane(title));
                set(&inst, "page", n(20.0));
                shot(
                    &inst,
                    window,
                    &layouts,
                    size,
                    &format!("{out}/home-type-{name}-{w}.png"),
                );
            }
        }
        set(&inst, "theme-mode", n(2.0));
        set(&inst, "detail", detail("風の中は走るっきゃないっ！"));
        set(&inst, "page", n(10.0));
        shot(
            &inst,
            window,
            &layouts,
            PhysicalSize::new(2300, 1190),
            &format!("{out}/light-album-type-2300.png"),
        );
        inst.hide().unwrap();
        return;
    }

    let only = std::env::var("UI_SNAPSHOT_WIDTHS").ok();
    for (w, h) in SIZES {
        if only
            .as_deref()
            .is_some_and(|o| !o.split(',').any(|x| x == w.to_string()))
        {
            continue;
        }
        let size = PhysicalSize::new(w, h);
        set(&inst, "full-open", Value::Bool(false));
        set(&inst, "page", n(20.0));
        shot(
            &inst,
            window,
            &layouts,
            size,
            &format!("{out}/home-{w}.png"),
        );
        shot(
            &inst,
            window,
            &layouts,
            PhysicalSize::new(w, 2700),
            &format!("{out}/home-full-{w}.png"),
        );
        set_g(&inst, "Home", "pane", pane(LONG_HERO));
        shot(
            &inst,
            window,
            &layouts,
            size,
            &format!("{out}/home-long-{w}.png"),
        );
        set_g(&inst, "Home", "pane", pane("PURE"));

        set(&inst, "page", n(0.0));
        shot(
            &inst,
            window,
            &layouts,
            size,
            &format!("{out}/albums-{w}.png"),
        );

        set(&inst, "page", n(10.0));
        shot(
            &inst,
            window,
            &layouts,
            size,
            &format!("{out}/album-{w}.png"),
        );
        set(&inst, "detail", detail(LONG_ALBUM));
        shot(
            &inst,
            window,
            &layouts,
            size,
            &format!("{out}/album-long-{w}.png"),
        );
        set(&inst, "detail", detail("DEKAI"));

        set(&inst, "full-open", Value::Bool(true));
        set(&inst, "full-tab", n(0.0));
        shot(
            &inst,
            window,
            &layouts,
            size,
            &format!("{out}/player-{w}.png"),
        );
        if w == 1280 {
            set(&inst, "np-album", s(LONG_ALBUM));
            shot(
                &inst,
                window,
                &layouts,
                size,
                &format!("{out}/player-long-{w}.png"),
            );
            set(&inst, "np-album", s("DEKAI"));
        }
        set(&inst, "full-open", Value::Bool(false));
    }
    if only.is_some() {
        inst.hide().unwrap();
        return;
    }

    // 接着听 shows when there are no daily mixes.
    set(&inst, "page", n(20.0));
    set_g(&inst, "Home", "mixes", model(Vec::new()));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(2300, 2700),
        &format!("{out}/home-resume-2300.png"),
    );
    set_g(&inst, "Home", "mixes", mixes());

    // Queue tab; the narrow overlay; the smallest window.
    set(&inst, "panel-queue-tab", Value::Bool(true));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(2300, 1190),
        &format!("{out}/home-queue-2300.png"),
    );
    set(&inst, "panel-queue-tab", Value::Bool(false));
    set(&inst, "overlay-open", Value::Bool(true));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(1280, 800),
        &format!("{out}/home-overlay-1280.png"),
    );
    set(&inst, "overlay-open", Value::Bool(false));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(960, 640),
        &format!("{out}/home-960.png"),
    );

    // Panel closed: the player bar shows the cover.
    set(&inst, "panel-open", Value::Bool(false));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(2300, 1190),
        &format!("{out}/home-nopanel-2300.png"),
    );
    set(&inst, "page", n(0.0));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(1600, 900),
        &format!("{out}/albums-nopanel-1600.png"),
    );
    set(&inst, "panel-open", Value::Bool(true));

    // Focused TopSearchBar on Search page (Option D chassis).
    set_g(&inst, "SearchState", "query-input", s("キタニタツヤ"));
    set_g(&inst, "SearchState", "active-query", s("キタニタツヤ"));
    set(&inst, "page", n(6.0));
    let _ = inst.invoke("focus-top-search", &[]);
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(1600, 900),
        &format!("{out}/search-focused-1600.png"),
    );
    set_g(&inst, "SearchState", "query-input", s(""));
    set_g(&inst, "SearchState", "active-query", s(""));
    set(&inst, "page", n(20.0));

    // 032 Light.
    set(&inst, "theme-mode", n(2.0));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(1600, 900),
        &format!("{out}/light-home-1600.png"),
    );
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(2300, 1190),
        &format!("{out}/light-home-2300.png"),
    );
    set(&inst, "page", n(10.0));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(2300, 1190),
        &format!("{out}/light-album-2300.png"),
    );
    set(&inst, "full-open", Value::Bool(true));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(2300, 1190),
        &format!("{out}/light-player-2300.png"),
    );
    set(&inst, "theme-mode", n(1.0));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(1700, 600),
        &format!("{out}/player-short-1700.png"),
    );
    set(&inst, "full-open", Value::Bool(false));
    set(&inst, "page", n(20.0));
    shot(
        &inst,
        window,
        &layouts,
        PhysicalSize::new(2560, 1440),
        &format!("{out}/home-4k150-2560.png"),
    );
    inst.hide().unwrap();
}
