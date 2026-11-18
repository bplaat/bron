/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use bcanvas::{
    CanvasImage, CanvasRenderingContext2d, Color, FontWeight, OffscreenCanvas, TextBaseline,
    premultiplied_bgra,
};
use bwindow::{
    Event, EventLoopBuilder, LogicalPoint, LogicalSize, Monitor, MouseButton, Theme, WindowBuilder,
    WindowEvent, open_url,
};
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::platforms::ScrollView;

mod platforms;

const FONT: &str = "Georgia";
const AVATAR_SIZE: f32 = 48.0;
const ICON_SIZE: f32 = 24.0;
// Georgia's ascent plus descent in ems, the height of a browser's inline text box
const TEXT_HEIGHT: f32 = 1.136;

// MARK: Palette
// The colors of the website: black on white, or white on black in dark mode
#[derive(Clone, Copy)]
struct Palette {
    background: u32,
    text: Color,
    // Links are the text color at 80% opacity
    link: Color,
    // The particles are this gray with alpha
    particle: u8,
}

impl Palette {
    const fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                background: 0xffffff,
                text: Color::rgb(0, 0, 0),
                link: Color::rgba(0, 0, 0, 204),
                particle: 0,
            },
            Theme::Dark => Self {
                background: 0x000000,
                text: Color::rgb(255, 255, 255),
                link: Color::rgba(255, 255, 255, 204),
                particle: 255,
            },
        }
    }
}

// MARK: Content
struct Link {
    text: &'static str,
    url: &'static str,
    icon: Option<&'static str>,
    dy: f32,
}

const fn link(text: &'static str, url: &'static str) -> Link {
    Link {
        text,
        url,
        icon: None,
        dy: 0.0,
    }
}

const fn icon_link(text: &'static str, url: &'static str, icon: &'static str, dy: f32) -> Link {
    Link {
        text,
        url,
        icon: Some(icon),
        dy,
    }
}

macro_rules! icons {
    ($($name:literal),*) => {
        [$(($name, include_bytes!(concat!("assets/", $name, ".tvg")).as_slice())),*]
    };
}

static ICONS: [(&str, &[u8]); 16] = icons!(
    "bassiemusic",
    "bible",
    "blog",
    "codepen",
    "coinlist",
    "facebook",
    "github",
    "instagram",
    "linkedin",
    "reddit",
    "redsquare",
    "soundcloud",
    "strepen",
    "warquest",
    "x",
    "youtube"
);

const AGE: &str = "{age} years old";

static FACTS: [Link; 7] = [
    link("Male human being", "https://en.wikipedia.org/wiki/Man"),
    link(AGE, "https://days.to/until/1-july"),
    link("Dutch", "https://en.wikipedia.org/wiki/Netherlands"),
    link("Protestant Christian", "https://www.oostpoort.net/"),
    link("Programming is my hobby", "https://github.com/bplaat"),
    link(
        "Volunteer by Scouting Die Kantanky's in Gouda, Netherlands",
        "https://www.diekantankys.nl/",
    ),
    link(
        "Member of Christen-Democratisch Appèl (CDA)",
        "https://cda.nl/",
    ),
];

static SITES: [Link; 14] = [
    link("plaatsoft.nl", "https://www.plaatsoft.nl/"),
    link("Bron des Codes", "https://github.com/bplaat/bron"),
    link("Strepen Systeem", "https://stam.diekantankys.nl/"),
    link("Neva Processor", "https://neva-processor.bplaat.nl/"),
    link("Coinlist", "https://coinlist.bplaat.nl/"),
    link("BassieMusic", "https://github.com/bplaat/bassiemusic"),
    link("WarQuest", "https://warquest.plaatsoft.nl/"),
    link("Battle of Teripa", "https://teripa.bplaat.nl/"),
    link("PlaatWorld", "https://plaatworld.bplaat.nl/"),
    link("PlaatWorld 3D", "https://plaatworld3d.bplaat.nl/"),
    link("Welpen Games", "https://welpen.bplaat.nl/"),
    link("PlaatGames", "https://plaatgames.bplaat.nl/"),
    link("Ruïnes game", "https://bplaat.itch.io/ruines"),
    link(
        "PlaatCraft",
        "https://github.com/bplaat/bron/tree/master/games/plaatcraft",
    ),
];

static APPS: [&[Link]; 2] = [
    &[
        icon_link(
            "BassieMusic",
            "https://play.google.com/store/apps/details?id=nl.plaatsoft.bassiemusic",
            "bassiemusic",
            1.0,
        ),
        icon_link(
            "Strepen",
            "https://play.google.com/store/apps/details?id=nl.plaatsoft.strepen",
            "strepen",
            0.0,
        ),
        icon_link(
            "Bible",
            "https://github.com/bplaat/android-apps/tree/master/bin/bible",
            "bible",
            0.0,
        ),
    ],
    &[
        icon_link(
            "CoinList",
            "https://github.com/bplaat/android-apps/tree/master/bin/coinlist",
            "coinlist",
            0.0,
        ),
        icon_link(
            "RedSquare",
            "https://www.plaatsoft.nl/android/redsquare/",
            "redsquare",
            0.0,
        ),
        icon_link(
            "WarQuest",
            "https://www.plaatsoft.nl/android/warquest/",
            "warquest",
            2.0,
        ),
    ],
];

static SOCIALS: [&[Link]; 3] = [
    &[
        icon_link("Blog", "https://blog.bplaat.nl/", "blog", 0.0),
        icon_link("GitHub", "https://github.com/bplaat", "github", 0.0),
        icon_link("CodePen", "https://codepen.io/bplaat", "codepen", 0.0),
        icon_link(
            "Linkedin",
            "https://www.linkedin.com/in/bplaat/",
            "linkedin",
            0.0,
        ),
    ],
    &[
        icon_link(
            "Facebook",
            "https://www.facebook.com/bplaat",
            "facebook",
            0.0,
        ),
        icon_link(
            "Instagram",
            "https://www.instagram.com/bplaat2/",
            "instagram",
            0.0,
        ),
        icon_link("Reddit", "https://reddit.com/u/bplaat", "reddit", 0.0),
    ],
    &[
        icon_link("X/Twitter", "https://x.com/bplaat2", "x", 0.0),
        icon_link("YouTube", "https://www.youtube.com/@bplaat", "youtube", 0.0),
        icon_link(
            "SoundCloud",
            "https://soundcloud.com/bplaat",
            "soundcloud",
            0.0,
        ),
    ],
];

// Headings and paragraphs have a 12px vertical margin, the lists have none
enum Block {
    Heading(&'static str),
    Text(&'static str, Option<&'static str>),
    Facts(&'static [Link]),
    Sites(&'static [Link]),
    Icons(&'static [Link]),
}

static BLOCKS: [Block; 13] = [
    Block::Heading("Bastiaan van der Plaat"),
    Block::Text("Some facts about me:", None),
    Block::Facts(&FACTS),
    Block::Text("Some sites and games I've made:", None),
    Block::Sites(&SITES),
    Block::Text("Some Android apps I've made:", None),
    Block::Icons(APPS[0]),
    Block::Icons(APPS[1]),
    Block::Text("You can also find me on:", None),
    Block::Icons(SOCIALS[0]),
    Block::Icons(SOCIALS[1]),
    Block::Icons(SOCIALS[2]),
    Block::Text("And my nickname around the internet is", Some("@bplaat")),
];

// MARK: Layout
#[derive(Clone, Copy, PartialEq)]
enum Font {
    Normal,
    Bold,
    Heading,
}

#[derive(Clone, Copy)]
enum Picture {
    Avatar,
    Icon(usize),
}

// A placed box: [pad][picture][gap][text][pad], in page coordinates
struct LayoutBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    font: Font,
    text: String,
    picture: Option<Picture>,
    dy: f32,
    pad: f32,
    gap: f32,
    link: Option<&'static Link>,
}

impl LayoutBox {
    fn text(font: Font, text: &str, link: Option<&'static Link>) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            font,
            text: text.to_string(),
            picture: None,
            dy: 0.0,
            pad: 0.0,
            gap: 0.0,
            link,
        }
    }

    // The height of the box's content like a browser's inline box: its text or picture and padding
    fn content_height(&self, font_size: f32) -> f32 {
        let text = match self.font {
            Font::Heading => font_size * 2.0,
            Font::Normal | Font::Bold => font_size,
        } * TEXT_HEIGHT;
        let picture = match self.picture {
            Some(Picture::Avatar) => AVATAR_SIZE,
            Some(Picture::Icon(_)) => ICON_SIZE,
            None => 0.0,
        };
        text.max(picture) + 2.0 * self.pad
    }
}

fn set_font(context: &mut CanvasRenderingContext2d<'_>, font: Font, size: f32) {
    let (size, weight) = match font {
        Font::Normal => (size, FontWeight::Normal),
        Font::Bold => (size, FontWeight::Bold),
        Font::Heading => (size * 2.0, FontWeight::Bold),
    };
    context.set_font(FONT, size);
    context.set_font_weight(weight);
}

// Inline flow: boxes are placed left to right, wrapped, and each line is centered
struct Flow<'a, 'b> {
    context: &'a mut CanvasRenderingContext2d<'b>,
    boxes: Vec<LayoutBox>,
    links: Vec<(&'static Link, [f32; 4])>,
    font_size: f32,
    left: f32,
    width: f32,
    line_height: f32,
    x: f32,
    y: f32,
    line_start: usize,
}

impl Flow<'_, '_> {
    const fn begin(&mut self, left: f32, width: f32, y: f32, line_height: f32) {
        self.left = left;
        self.width = width;
        self.line_height = line_height;
        self.x = 0.0;
        self.y = y;
        self.line_start = self.boxes.len();
    }

    fn line(&mut self) {
        if self.boxes.len() == self.line_start {
            return;
        }
        // Boxes start on whole pixels, where pictures copy and fills skip antialiasing
        let shift = self.left + (self.width - self.x) / 2.0;
        for layout_box in &mut self.boxes[self.line_start..] {
            layout_box.x = (layout_box.x + shift).round();
        }
        // Like a browser, a link hits on the rectangle of its words on the line with the spaces between
        // them, as high as its content and centered on the line
        let center = self.y + self.line_height / 2.0;
        let mut boxes = self.boxes[self.line_start..].iter().peekable();
        while let Some(first) = boxes.next() {
            let Some(link) = first.link else {
                continue;
            };
            let mut right = first.x + first.width;
            let mut height = first.content_height(self.font_size);
            while let Some(next) =
                boxes.next_if(|next| next.link.is_some_and(|next| std::ptr::eq(next, link)))
            {
                right = next.x + next.width;
                height = height.max(next.content_height(self.font_size));
            }
            self.links.push((
                link,
                [first.x, center - height / 2.0, right - first.x, height],
            ));
        }
        self.y += self.line_height;
        self.x = 0.0;
        self.line_start = self.boxes.len();
    }

    fn space(&mut self, font: Font) -> f32 {
        self.measure(font, " ")
    }

    fn measure(&mut self, font: Font, text: &str) -> f32 {
        set_font(self.context, font, self.font_size);
        self.context.measure_text(text)
    }

    // The glue is the spacing before the box, it is dropped at the start of a line
    fn push(&mut self, glue: f32, mut layout_box: LayoutBox) {
        let picture = match layout_box.picture {
            Some(Picture::Avatar) => AVATAR_SIZE + layout_box.gap,
            Some(Picture::Icon(_)) => ICON_SIZE + layout_box.gap,
            None => 0.0,
        };
        let text = if layout_box.text.is_empty() {
            0.0
        } else {
            self.measure(layout_box.font, &layout_box.text)
        };
        layout_box.width = 2.0 * layout_box.pad + picture + text;
        if self.x > 0.0 && self.x + glue + layout_box.width > self.width {
            self.line();
        }
        if self.x > 0.0 {
            self.x += glue;
        }
        layout_box.x = self.x;
        layout_box.y = self.y;
        layout_box.height = self.line_height;
        self.x += layout_box.width;
        self.boxes.push(layout_box);
    }

    fn words(&mut self, mut glue: f32, font: Font, text: &str, link: Option<&'static Link>) {
        let space = self.space(font);
        for word in text.split(' ') {
            self.push(glue, LayoutBox::text(font, word, link));
            glue = space;
        }
    }
}

struct Layout {
    boxes: Vec<LayoutBox>,
    links: Vec<(&'static Link, [f32; 4])>,
    font_size: f32,
    width: f32,
    height: f32,
}

fn layout(context: &mut CanvasRenderingContext2d<'_>, width: f32, age: &str) -> Layout {
    let font_size = if width >= 768.0 { 20.0 } else { 16.0 };
    let line_height = font_size * 1.5;
    let container_width = width.clamp(350.0, 800.0);
    let container_x = ((width - container_width) / 2.0).max(0.0);
    let list_x = container_x + container_width * 0.15;
    let list_width = container_width * 0.7;
    let mut flow = Flow {
        context,
        boxes: Vec::new(),
        links: Vec::new(),
        font_size,
        left: 0.0,
        width: 0.0,
        line_height,
        x: 0.0,
        y: 0.0,
        line_start: 0,
    };
    let (mut y, mut margin) = (0.0f32, 0.0f32);
    for block in &BLOCKS {
        let block_margin = if matches!(block, Block::Heading(_) | Block::Text(..)) {
            12.0
        } else {
            0.0
        };
        y += margin.max(block_margin);
        margin = block_margin;
        match *block {
            Block::Heading(text) => {
                flow.begin(container_x, container_width, y, font_size * 3.0);
                flow.push(
                    0.0,
                    LayoutBox {
                        picture: Some(Picture::Avatar),
                        dy: -4.0,
                        ..LayoutBox::text(Font::Normal, "", None)
                    },
                );
                let glue = 4.0 + flow.space(Font::Heading);
                flow.words(glue, Font::Heading, text, None);
            }
            Block::Text(text, bold) => {
                flow.begin(container_x, container_width, y, line_height);
                flow.words(0.0, Font::Normal, text, None);
                if let Some(bold) = bold {
                    let glue = flow.space(Font::Normal);
                    flow.words(glue, Font::Bold, bold, None);
                }
            }
            Block::Facts(links) => {
                flow.begin(list_x, list_width, y, line_height);
                let glue = flow.space(Font::Normal) + 6.0;
                for (index, link) in links.iter().enumerate() {
                    if index > 0 {
                        flow.words(glue, Font::Normal, "-", None);
                    }
                    let text = link.text.replace("{age}", age);
                    flow.words(
                        if index > 0 { glue } else { 0.0 },
                        Font::Bold,
                        &text,
                        Some(link),
                    );
                }
            }
            // Two columns, one link per line
            Block::Sites(links) => {
                let column_width = 300.0f32.min(container_width / 2.0);
                let half = links.len() / 2;
                let mut bottom = y;
                for (column, links) in links.chunks(half).enumerate() {
                    let left = container_x
                        + (container_width - 2.0 * column_width) / 2.0
                        + column as f32 * column_width;
                    flow.begin(left, column_width, y, line_height);
                    for link in links {
                        flow.words(0.0, Font::Bold, link.text, Some(link));
                        flow.line();
                    }
                    bottom = bottom.max(flow.y);
                }
                y = bottom;
                continue;
            }
            Block::Icons(links) => {
                flow.begin(list_x, list_width, y, line_height);
                let glue = flow.space(Font::Normal) + 12.0;
                for (index, link) in links.iter().enumerate() {
                    let icon = ICONS.iter().position(|(name, _)| Some(*name) == link.icon);
                    flow.push(
                        if index > 0 { glue } else { 0.0 },
                        LayoutBox {
                            picture: icon.map(Picture::Icon),
                            dy: link.dy,
                            pad: 4.0,
                            gap: 8.0,
                            ..LayoutBox::text(Font::Bold, link.text, Some(link))
                        },
                    );
                }
            }
        }
        flow.line();
        y = flow.y;
    }
    Layout {
        boxes: flow.boxes,
        links: flow.links,
        font_size,
        width,
        height: (y + margin).ceil(),
    }
}

// MARK: Particles
// One particle per this many square logical pixels, Windows XP draws them in software so it gets a quarter
const PARTICLE_AREA: f32 = if cfg!(target_vendor = "winxp") {
    4000.0
} else {
    1000.0
};

// A xorshift generator, the particles only need to look random
struct Random(u64);

impl Random {
    fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |time| time.as_nanos() as u64);
        Self(seed | 1)
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        min + (max - min) * ((self.0 >> 40) as f32 / (1u64 << 24) as f32)
    }
}

// Positions are relative to the window center, like the website's background
struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    size: f32,
    alpha: u8,
}

impl Particle {
    fn new(random: &mut Random, size: LogicalSize, start: bool) -> Self {
        let (width, height) = (size.width, size.height);
        Self {
            x: random.range(-width, if start { width } else { -width / 2.0 }),
            y: random.range(-height, if start { height } else { height * 2.0 }),
            vx: random.range(0.5, 1.5),
            vy: random.range(-1.5, -0.5),
            size: random.range(0.5, 3.0),
            alpha: (random.range(0.2, 0.6) * 255.0) as u8,
        }
    }
}

// MARK: App
struct App {
    size: LogicalSize,
    palette: Palette,
    // The avatar at 1x and 2x
    avatars: [CanvasImage; 2],
    icons: Vec<OffscreenCanvas>,
    layout: Option<Layout>,
    hover: Option<&'static str>,
    pressed: Option<&'static str>,
    random: Random,
    particles: Vec<Particle>,
    tick: Instant,
}

impl App {
    fn new(size: LogicalSize, theme: Theme) -> Self {
        let decode = |data: &[u8]| {
            let image = image::decode(data).expect("the avatar is a valid PNG");
            let bitmap = image.largest_frame().bitmap();
            CanvasImage::from_premultiplied_bgra(
                bitmap.width(),
                bitmap.height(),
                premultiplied_bgra(bitmap.data()),
            )
        };
        Self {
            size,
            palette: Palette::for_theme(theme),
            avatars: [
                decode(include_bytes!("assets/avatar.png")),
                decode(include_bytes!("assets/avatar@2x.png")),
            ],
            icons: ICONS
                .iter()
                .map(|(_, data)| vector::decode_vector(data).expect("the icons are valid TinyVG"))
                .collect(),
            layout: None,
            hover: None,
            pressed: None,
            random: Random::new(),
            particles: Vec::new(),
            tick: Instant::now(),
        }
    }

    fn resize(&mut self, size: LogicalSize) {
        self.size = size;
        self.layout = None;
        let count = (size.width * size.height / PARTICLE_AREA) as usize;
        self.particles.truncate(count);
        while self.particles.len() < count {
            let particle = Particle::new(&mut self.random, size, true);
            self.particles.push(particle);
        }
    }

    fn content_height(&self) -> f32 {
        self.layout.as_ref().map_or(0.0, |layout| layout.height)
    }

    // The page's top in the view at a scroll position, it is centered when it fits
    fn offset(&self, scroll: f32) -> f32 {
        ((self.size.height - self.content_height()) / 2.0)
            .max(0.0)
            .round()
            - scroll
    }

    fn link_at(&self, point: LogicalPoint, scroll: f32) -> Option<&'static str> {
        let (x, y) = (point.x, point.y - self.offset(scroll));
        let layout = self.layout.as_ref()?;
        layout
            .links
            .iter()
            .find(|(_, [left, top, width, height])| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(link, _)| link.url)
    }

    fn step(&mut self) {
        let now = Instant::now();
        let dt = (now - self.tick).as_secs_f32().min(0.25);
        self.tick = now;
        let size = self.size;
        for particle in &mut self.particles {
            particle.vx = (particle.vx + self.random.range(-0.05, 0.05)).clamp(0.3, 2.0);
            particle.vy = (particle.vy + self.random.range(-0.05, 0.05)).clamp(-2.0, -0.3);
            particle.x += particle.vx * 30.0 * dt;
            particle.y += particle.vy * 30.0 * dt;
            if particle.x > size.width || particle.y < -size.height {
                *particle = Particle::new(&mut self.random, size, false);
            }
        }
    }

    fn update_layout(&mut self, context: &mut CanvasRenderingContext2d<'_>) {
        if self
            .layout
            .as_ref()
            .is_none_or(|layout| layout.width != self.size.width)
        {
            let today = Timestamp::now().to_zoned_datetime(TimeZone::UTC).date();
            let age = today.year() - 2002 - i16::from(today.month() < 7);
            self.layout = Some(layout(context, self.size.width, &age.to_string()));
        }
    }

    fn draw(&mut self, context: &mut CanvasRenderingContext2d<'_>, scroll: f32) {
        self.size = LogicalSize::new(context.width(), context.height());
        self.update_layout(context);
        let palette = self.palette;
        let background = palette.background;
        context.set_fill_style(Color::rgb(
            (background >> 16) as u8,
            (background >> 8) as u8,
            background as u8,
        ));
        context.fill_rect(0.0, 0.0, self.size.width, self.size.height);

        let (center_x, center_y) = (self.size.width / 2.0, self.size.height / 2.0);
        for particle in &self.particles {
            let base = palette.particle;
            context.set_fill_style(Color::rgba(base, base, base, particle.alpha));
            context.begin_path();
            context.ellipse(
                center_x + particle.x,
                center_y + particle.y,
                particle.size,
                particle.size,
            );
            context.fill();
        }

        let Some(layout) = &self.layout else {
            return;
        };
        let offset = self.offset(scroll);
        let avatar = &self.avatars[usize::from(context.scale_factor() > 1.0)];
        context.set_text_baseline(TextBaseline::Middle);
        for layout_box in &layout.boxes {
            // Skips the boxes outside the view
            let top = offset + layout_box.y;
            if top + layout_box.height < 0.0 || top > self.size.height {
                continue;
            }
            let hovered = layout_box
                .link
                .is_some_and(|link| Some(link.url) == self.hover);
            let color = if layout_box.link.is_some() && !hovered {
                palette.link
            } else {
                palette.text
            };
            let mut x = layout_box.x + layout_box.pad;
            let center = top + layout_box.height / 2.0;
            match layout_box.picture {
                Some(Picture::Avatar) => {
                    let y = center - AVATAR_SIZE / 2.0 + layout_box.dy;
                    context.draw_image(avatar, x, y, AVATAR_SIZE, AVATAR_SIZE);
                    x += AVATAR_SIZE + layout_box.gap;
                }
                // The icon masks the color, so it follows the text like the website's, the clip
                // keeps the layer as small as the icon
                Some(Picture::Icon(index)) => {
                    let y = center - ICON_SIZE / 2.0 + layout_box.dy;
                    context.save();
                    context.begin_path();
                    context.rect(x, y, ICON_SIZE, ICON_SIZE);
                    context.clip();
                    context.begin_mask_layer();
                    context.draw_canvas(&self.icons[index], x, y, ICON_SIZE, ICON_SIZE);
                    context.end_mask();
                    context.set_fill_style(color);
                    context.fill_rect(x, y, ICON_SIZE, ICON_SIZE);
                    context.end_layer();
                    context.restore();
                    x += ICON_SIZE + layout_box.gap;
                }
                None => {}
            }
            if !layout_box.text.is_empty() {
                set_font(context, layout_box.font, layout.font_size);
                context.set_fill_style(color);
                context.fill_text(&layout_box.text, x, center);
            }
        }
    }
}

// MARK: Window placement
// 90% of the monitor area without the menu bar, Dock or taskbar
fn placed_size(monitor: &Monitor) -> LogicalSize {
    let (size, safe_area) = (monitor.size(), monitor.safe_area());
    LogicalSize::new(
        (size.width - safe_area.left - safe_area.right) * 0.9,
        (size.height - safe_area.top - safe_area.bottom) * 0.9,
    )
}

fn main() {
    let event_loop = EventLoopBuilder::new()
        .app_id("nl", "bplaat", "Bastiaan")
        .build();
    let monitor = event_loop.primary_monitor();
    let mut window = WindowBuilder::new()
        .title("Bastiaan van der Plaat")
        .size(placed_size(&monitor))
        .min_size(LogicalSize::new(400.0, 300.0))
        .background_color(Palette::for_theme(event_loop.theme()).background)
        .center()
        .build();
    let window_id = window.id();
    let view = ScrollView::new(&window);
    view.set_dark(event_loop.theme() == Theme::Dark);
    let mut app = App::new(window.size(), event_loop.theme());
    app.resize(window.size());
    // Until the user resizes or maximizes the window, a changed screen places it again, like the screen size
    // Windows XP follows from the browser. Only the size tells, Windows moves windows along with a new resolution
    let mut placed = window.size();

    event_loop.run(move |event| match event {
        Event::Window(id, event) if id == window_id => match event {
            // The view scrolls by itself, it draws at its scroll position and scrolls through
            // the page that the drawing laid out
            WindowEvent::RedrawRequested => {
                // Every painted frame steps the particles and asks for the next display frame
                app.step();
                let scroll = view.scroll();
                if view.draw(|context| app.draw(context, scroll)) {
                    view.set_content_height(app.content_height());
                }
                view.request_animation_frame();
            }
            WindowEvent::Resize(size) => {
                app.resize(size);
                view.request_redraw();
            }
            WindowEvent::ScaleFactorChanged(_) => view.request_redraw(),
            WindowEvent::MonitorChanged => {
                let size = window.size();
                if (size.width - placed.width).abs() < 1.0
                    && (size.height - placed.height).abs() < 1.0
                {
                    window.set_size(placed_size(&monitor));
                    window.center();
                    placed = window.size();
                }
            }
            WindowEvent::ThemeChanged(theme) => {
                app.palette = Palette::for_theme(theme);
                window.set_background_color(app.palette.background);
                view.set_dark(theme == Theme::Dark);
                view.request_redraw();
            }
            WindowEvent::MouseMove { position, .. } => {
                let hover = app.link_at(position, view.scroll());
                view.set_pointer_cursor(hover.is_some());
                if hover != app.hover {
                    app.hover = hover;
                    view.request_redraw();
                }
            }
            WindowEvent::MouseLeave => {
                if app.hover.take().is_some() {
                    view.request_redraw();
                }
            }
            WindowEvent::MouseDown(event) if event.button == MouseButton::Left => {
                app.pressed = app.link_at(event.position, view.scroll());
            }
            WindowEvent::MouseUp(event) if event.button == MouseButton::Left => {
                if let Some(url) = app.pressed.take()
                    && app.link_at(event.position, view.scroll()) == Some(url)
                {
                    open_url(url);
                }
            }
            _ => {}
        },
        _ => {}
    });
}
