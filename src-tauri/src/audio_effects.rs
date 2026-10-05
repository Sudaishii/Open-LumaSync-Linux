use crate::hid::LedColor;

pub const MODES: &[&str] = &["spectrum", "energy", "beat", "bounce", "comet", "twin_bounce", "ripple", "vu", "wave", "pulse", "swell", "spark", "prism", "tremor", "orbit"];
pub const PALETTES: &[&str] = &["rainbow", "aurora", "sunset", "ocean", "neon", "ember", "forest", "candy", "custom", "selected"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Palette { Rainbow, Aurora, Sunset, Ocean, Neon, Ember, Forest, Candy, Custom, Selected }

impl Palette {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "rainbow" => Ok(Self::Rainbow), "aurora" => Ok(Self::Aurora),
            "sunset" => Ok(Self::Sunset), "ocean" => Ok(Self::Ocean),
            "neon" => Ok(Self::Neon), "ember" => Ok(Self::Ember),
            "forest" => Ok(Self::Forest), "candy" => Ok(Self::Candy),
            "custom" => Ok(Self::Custom), "selected" => Ok(Self::Selected),
            _ => Err("Choose a listed audio color palette.".into()),
        }
    }
}

#[derive(Clone)]
pub struct Options { pub palette: Palette, pub secondary: LedColor, pub speed: f64, pub width: f64, pub noise_gate: f64, pub reverse: bool }
impl Default for Options {
    fn default() -> Self {
        Self {palette: Palette::Rainbow, secondary: LedColor {r:171,g:148,b:210}, speed:4., width:0.24, noise_gate:0., reverse:false}
    }
}

#[derive(Default)]
pub struct Renderer { phase: f64 }
impl Renderer {
    pub fn render(&mut self, mode: &str, count: usize, level: f64, bands: &[f64], primary: &LedColor, options: &Options, dt: f64) -> Vec<LedColor> {
        if count==0 { return Vec::new(); }
        let level=if level<=options.noise_gate {0.} else {level.clamp(0.,1.)};
        if level>0.002 { self.phase += dt.clamp(0.,0.25)*options.speed*0.25*(0.2+0.8*level); }
        let center = bounce_position(self.phase)*(count-1) as f64;
        let radius = (count as f64*options.width*0.5).max(2.);
        let mut colors: Vec<LedColor> = (0..count).map(|i| {
            if level==0. { return LedColor::default(); }
            if count==1 { return scale(&palette_color(options.palette,primary,&options.secondary,self.phase*0.1),level); }
            let position = if count==1 {0.5} else {i as f64/(count-1) as f64};
            let (intensity, palette_position) = match mode {
                "spectrum" => {
                    if bands.is_empty() { return LedColor::default(); }
                    let band = position*(bands.len()-1) as f64;
                    let lo = band.floor() as usize;
                    let hi = (lo+1).min(bands.len()-1);
                    let value = bands[lo]*(1.-band.fract())+bands[hi]*band.fract();
                    (value,position+self.phase*0.1)
                }
                "energy" => (level,position+self.phase*0.1),
                "beat" => {
                    let distance = (position-0.5).abs()*2.;
                    let extent = level.sqrt();
                    let intensity = if extent>0.002 {(1.-distance/extent).clamp(0.,1.)*level} else {0.};
                    (intensity,position+self.phase*0.1)
                }
                "bounce" => {
                    let local = (i as f64-center)/radius;
                    ((1.-local.abs()).clamp(0.,1.)*level,(local+1.)*0.5+self.phase*0.1)
                }
                "twin_bounce" => {
                    let distance=(i as f64-center).abs().min((i as f64-((count-1) as f64-center)).abs());
                    ((1.-distance/radius).clamp(0.,1.)*level,position+self.phase*0.1)
                }
                "comet" => {
                    let head=self.phase.rem_euclid(1.);
                    let distance=(head-position).rem_euclid(1.);
                    ((1.-distance/options.width).clamp(0.,1.)*level,position+self.phase*0.1)
                }
                "ripple" => {
                    let distance=(position-0.5).abs()*2.;
                    let ring=self.phase.rem_euclid(1.);
                    ((1.-(distance-ring).abs()/options.width).clamp(0.,1.)*level,position+self.phase*0.1)
                }
                "vu" => {
                    let distance=(position-0.5).abs()*2.;
                    let edge=(2./count as f64).max(0.03);
                    (((level-distance)/edge+1.).clamp(0.,1.)*level,position+self.phase*0.1)
                }
                "wave" => {
                    let wave=(0.5+0.5*((position/options.width-self.phase)*std::f64::consts::TAU).sin()).powi(2);
                    ((0.08+0.92*wave)*level,position+self.phase*0.1)
                }
                "pulse" => {
                    let envelope=(0.5+0.5*((self.phase*std::f64::consts::TAU).sin())).powi(2);
                    (envelope*level,position+self.phase*0.08)
                }
                "swell" => {
                    let center=(0.5+0.5*((self.phase*std::f64::consts::TAU).sin())).clamp(0.,1.);
                    let width=options.width.max(0.08);
                    (((1.-(position-center).abs()/width).clamp(0.,1.)*0.8+0.2)*level,position+self.phase*0.12)
                }
                "spark" => {
                    let cell=((position*97.0).floor()+self.phase*options.speed*3.0).floor();
                    let noise=((cell*12.9898).sin()*43758.5453).fract().abs();
                    (((noise-0.55).max(0.)/0.45*0.9+0.1)*level,position+self.phase*0.14)
                }
                "prism" => {
                    let shimmer=0.55+0.45*((position*std::f64::consts::TAU*2.0+self.phase*2.0).sin());
                    (shimmer*level,position*1.7+self.phase*0.18)
                }
                "tremor" => {
                    let wobble=(0.65+0.35*((position*std::f64::consts::TAU*4.0-self.phase*3.0).sin())).clamp(0.,1.);
                    (wobble*level,position+self.phase*0.1)
                }
                "orbit" => {
                    let head=(self.phase*0.5).rem_euclid(1.);
                    let distance=(head-position).abs().min(1.-(head-position).abs());
                    ((1.-distance/options.width.max(0.08)).clamp(0.,1.)*level,position+self.phase*0.2)
                }
                _ => return LedColor::default(),
            };
            let c = palette_color(options.palette,primary,&options.secondary,palette_position);
            scale(&c,intensity.clamp(0.,1.))
        }).collect();
        if options.reverse { colors.reverse(); }
        colors
    }
}

fn bounce_position(phase: f64) -> f64 {
    let p=phase.rem_euclid(2.);
    if p<=1. {p} else {2.-p}
}
pub fn brightness_response(intensity: f64) -> f64 {
    // Lift quiet signals into a visible LED range without raising the user's
    // brightness ceiling or lighting the strip during silence.
    intensity.clamp(0., 1.).sqrt()
}
fn scale(c: &LedColor, intensity: f64) -> LedColor {
    let intensity = brightness_response(intensity);
    LedColor {r:(c.r as f64*intensity).round() as u8,g:(c.g as f64*intensity).round() as u8,b:(c.b as f64*intensity).round() as u8}
}
fn blend(a: &LedColor,b: &LedColor,t: f64) -> LedColor {
    LedColor {r:(a.r as f64+(b.r as f64-a.r as f64)*t).round() as u8,g:(a.g as f64+(b.g as f64-a.g as f64)*t).round() as u8,b:(a.b as f64+(b.b as f64-a.b as f64)*t).round() as u8}
}
pub fn palette_color(palette: Palette,primary: &LedColor,secondary: &LedColor,position: f64) -> LedColor {
    if palette==Palette::Selected {return primary.clone();}
    let points: Vec<LedColor> = match palette {
        Palette::Rainbow => vec![LedColor {r:255,g:0,b:0},LedColor {r:255,g:220,b:0},LedColor {r:0,g:255,b:90},LedColor {r:0,g:210,b:255},LedColor {r:100,g:55,b:255},LedColor {r:255,g:0,b:180}],
        Palette::Aurora => vec![LedColor {r:0,g:230,b:170},LedColor {r:70,g:140,b:255},LedColor {r:190,g:65,b:255}],
        Palette::Sunset => vec![LedColor {r:255,g:125,b:30},LedColor {r:255,g:55,b:110},LedColor {r:130,g:50,b:235}],
        Palette::Ocean => vec![LedColor {r:0,g:225,b:240},LedColor {r:15,g:70,b:255},LedColor {r:90,g:175,b:255}],
        Palette::Neon => vec![LedColor {r:255,g:0,b:150},LedColor {r:0,g:255,b:235},LedColor {r:180,g:255,b:0}],
        Palette::Ember => vec![LedColor {r:255,g:25,b:0},LedColor {r:255,g:120,b:0},LedColor {r:255,g:220,b:65}],
        Palette::Forest => vec![LedColor {r:20,g:150,b:65},LedColor {r:150,g:255,b:20},LedColor {r:0,g:210,b:150}],
        Palette::Candy => vec![LedColor {r:255,g:90,b:175},LedColor {r:150,g:100,b:255},LedColor {r:65,g:210,b:255}],
        Palette::Custom => vec![primary.clone(),secondary.clone()],
        Palette::Selected => unreachable!(),
    };
    let index = position.rem_euclid(1.)*points.len() as f64;
    let lo=index.floor() as usize;
    blend(&points[lo],&points[(lo+1)%points.len()],index.fract())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn primary() -> LedColor {LedColor {r:255,g:80,b:10}}
    fn peak(colors: &[LedColor]) -> usize {
        colors.iter().enumerate().max_by_key(|(_,c)|c.r as u16+c.g as u16+c.b as u16).unwrap().0
    }
    #[test]
    fn every_style_respects_count_silence_and_multicolor_palettes() {
        for mode in MODES {for palette in PALETTES {for count in [1,54,254] {
            let options=Options {palette:Palette::parse(palette).unwrap(),..Default::default()};
            let mut renderer=Renderer::default();
            let silent=renderer.render(mode,count,0.,&[0.;8],&primary(),&options,0.1);
            assert_eq!(silent.len(),count);
            assert!(silent.iter().all(|c|c.r==0&&c.g==0&&c.b==0),"{mode}/{palette} lit in silence");
            let active=renderer.render(mode,count,0.8,&[0.8;8],&primary(),&options,0.1);
            assert!(active.iter().any(|c|c.r>0||c.g>0||c.b>0),"{mode}/{palette} stayed dark");
            if count>1&&*palette!="selected" {
                let mut hues=std::collections::HashSet::new();
                for c in active.iter().filter(|c|c.r as u16+c.g as u16+c.b as u16>10) {
                    let max=c.r.max(c.g).max(c.b) as u16;
                    hues.insert((c.r as u16*100/max,c.g as u16*100/max,c.b as u16*100/max));
                }
                assert!(hues.len()>1,"{mode}/{palette} only produced one hue");
            }
        }}}
    }
    #[test]
    fn bounce_reaches_both_ends_and_reverses_on_audio() {
        let options=Options {palette:Palette::Selected,..Default::default()};
        let mut renderer=Renderer {phase:0.};
        assert_eq!(peak(&renderer.render("bounce",54,1.,&[1.;8],&primary(),&options,0.)),0);
        renderer.phase=1.;
        assert_eq!(peak(&renderer.render("bounce",54,1.,&[1.;8],&primary(),&options,0.)),53);
        renderer.phase=2.;
        assert_eq!(peak(&renderer.render("bounce",54,1.,&[1.;8],&primary(),&options,0.)),0);
        let phase=renderer.phase;
        renderer.render("bounce",54,0.,&[0.;8],&primary(),&options,0.25);
        assert_eq!(renderer.phase,phase,"silent input must not advance bounce");
        renderer.render("bounce",54,0.5,&[0.5;8],&primary(),&options,0.25);
        assert!(renderer.phase>phase);
    }
    #[test]
    fn custom_blend_uses_both_user_colors_and_rejects_unknown_palettes() {
        let a=LedColor {r:255,g:0,b:0};let b=LedColor {r:0,g:0,b:255};
        let red=palette_color(Palette::Custom,&a,&b,0.);
        let blue=palette_color(Palette::Custom,&a,&b,0.5);
        assert_eq!((red.r,red.g,red.b),(255,0,0));
        assert_eq!((blue.r,blue.g,blue.b),(0,0,255));
        assert!(Palette::parse("bad").is_err());
    }
    #[test]
    fn gate_reverse_and_width_change_real_frames() {
        let mut options=Options {noise_gate:0.2,..Default::default()};
        for mode in MODES {
            let silent=Renderer::default().render(mode,54,0.1,&[1.;8],&primary(),&options,0.1);
            assert!(silent.iter().all(|c|c.r==0&&c.g==0&&c.b==0),"{mode} ignored gate");
            let forward=Renderer::default().render(mode,54,0.8,&[0.8;8],&primary(),&options,0.1);
            options.reverse=true;
            let backward=Renderer::default().render(mode,54,0.8,&[0.8;8],&primary(),&options,0.1);
            assert!(forward.iter().rev().zip(&backward).all(|(a,b)|(a.r,a.g,a.b)==(b.r,b.g,b.b)),"{mode} ignored reverse");
            options.reverse=false;
        }
        for mode in ["bounce","twin_bounce","comet","ripple","wave"] {
            options.width=0.08;
            let narrow=Renderer::default().render(mode,54,0.8,&[0.8;8],&primary(),&options,0.1);
            options.width=0.8;
            let wide=Renderer::default().render(mode,54,0.8,&[0.8;8],&primary(),&options,0.1);
            assert!(narrow.iter().zip(wide).any(|(a,b)|(a.r,a.g,a.b)!=(b.r,b.g,b.b)),"{mode} ignored width");
        }
    }

    #[test]
    fn quiet_background_music_produces_a_visible_volume_frame() {
        let options = Options {palette: Palette::Selected, ..Default::default()};
        // An audible, low-volume signal must stay visible instead of being
        // multiplied down to a handful of RGB values.
        for mode in ["energy", "bounce", "beat", "vu"] {
            let colors = Renderer::default().render(mode, 54, 0.08, &[0.08;8], &primary(), &options, 1./30.);
            let peak = colors.iter().map(|c| c.r.max(c.g).max(c.b)).max().unwrap();
            assert!(peak >= 60, "{mode} made quiet music barely visible: peak={peak}");
        }
    }
}
