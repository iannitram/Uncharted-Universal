//! The mouse camera of `src/runtime_pad.c`: the follow, cover and aiming camera hooks and the
//! mouse as a right stick, over plain data (the guest memory is read and written by `hooks`).
//!
//! The games turn the camera from the right stick through their own curve, a speed ramp and a
//! top speed, which no stick position can make feel like a mouse. Where the camera's state is
//! known, the mouse turns it directly instead: a callback at the start of the camera's
//! per-frame update rotates its direction by the mouse motion since the last frame. Elsewhere
//! the mouse still moves the right stick. All of this state sits behind one lock (the C
//! `camera_lock`, widened to the fields the callbacks shared without one).

use super::note;

/// Degrees of camera turn per mouse pixel at sensitivity 1 (0.08 felt "very slow", issue #18;
/// the launcher's slider and box go from 0.1 to 10 times this).
pub const MOUSE_DEGREES_PER_PIXEL: f32 = 0.15;
const RADIANS: f32 = 0.017453293;

/// The settings of `input.ini` the camera needs.
#[derive(Clone, Copy, Debug)]
pub struct CameraInput {
    pub mouse_sensitivity: f32,
    /// Raw mouse (Uncharted 1 and 2): aiming, cover and blending cameras turn 1:1.
    pub mouse_raw: bool,
    /// Raw aiming: times `mouse_sensitivity`.
    pub mouse_aim_sensitivity: f32,
    pub mouse_invert_y: bool,
}

impl Default for CameraInput {
    fn default() -> Self {
        CameraInput {
            mouse_sensitivity: 1.0,
            mouse_raw: false,
            mouse_aim_sensitivity: 1.0,
            mouse_invert_y: false,
        }
    }
}

/// Measuring knobs (`BB_MOUSE_*` environment), read once.
#[derive(Clone, Copy, Debug, Default)]
pub struct Knobs {
    pub debug: bool,
    /// `BB_MOUSE_CAMERA_TEST=<degrees per frame>`: constant turn.
    pub camera_test_rate: f32,
    /// `BB_MOUSE_FOLLOW_TEST=<x>`: right stick x everywhere.
    pub follow_test: f32,
    /// `BB_MOUSE_AIM_TEST=<x>`: right stick x while not following.
    pub aim_test: f32,
    /// `BB_MOUSE_CAL=<pixels/s per stick unit>`: the mouse is a raw stick value.
    pub cal_scale: f32,
}

impl Knobs {
    pub fn from_env() -> Knobs {
        let number = |name: &str| {
            std::env::var(name)
                .ok()
                .map_or(0.0, |text| super::input::atof(&text) as f32)
        };
        Knobs {
            debug: std::env::var_os("BB_MOUSE_DEBUG").is_some(),
            camera_test_rate: number("BB_MOUSE_CAMERA_TEST"),
            follow_test: number("BB_MOUSE_FOLLOW_TEST"),
            aim_test: number("BB_MOUSE_AIM_TEST"),
            cal_scale: number("BB_MOUSE_CAL"),
        }
    }
}

/// A turn rate (degrees/s) as a stick byte, through the inverse of the U1 follow camera's
/// response measured below its speed ramp: about 240 x^2.5 degrees/s (x = 0.25: 7, 0.5: 37).
pub fn mouse_axis(rate: f32) -> u8 {
    let r = rate.abs();
    if r < 0.5 {
        return 128;
    }
    let out = (r / 240.0).powf(0.4).min(1.0);
    let x = (128.0 + if rate < 0.0 { -out } else { out } * 127.5) as i32;
    x.clamp(0, 255) as u8
}

pub fn wrap_degrees(mut a: f32) -> f32 {
    while a > 180.0 {
        a -= 360.0;
    }
    while a < -180.0 {
        a += 360.0;
    }
    a
}

pub fn heading_of(dir: &[f32; 3]) -> f32 {
    (-dir[0]).atan2(-dir[2]) * 57.29578
}

/// One follow camera's turn state. Without raw mouse there is one, shared (slot 0). With it
/// each camera has its own: while the game blends (a roll, cover, a cut) it runs two follow
/// cameras, a new one each time, and shared state made them pull against each other.
#[derive(Clone, Copy, Debug, Default)]
pub struct FollowState {
    pub camera: u64,
    pub last_us: u64,
    /// Last frame the mouse moved.
    pub moved_us: u64,
    /// Wanted heading (degrees).
    pub target: f32,
    /// Heading before and after the last turn.
    pub before: f32,
    pub after: f32,
    /// Raw mouse: where the game's smoothed camera is, estimated (degrees, degrees/s).
    pub shown: f32,
    pub shown_v: f32,
    pub shown_us: u64,
    pub pending: bool,
}

/// The follow camera's vertical arc (0..1) against the pitch of the camera the game shows
/// (degrees, up is positive), measured with slow mouse sweeps: the arc is far from linear.
/// Outside the table the end slopes continue.
pub struct ArcTable {
    pub arc: &'static [f32],
    pub pitch: &'static [f32],
}

pub static U1_ARC: ArcTable = ArcTable {
    arc: &[0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0],
    pitch: &[
        0.6, -1.5, -4.7, -7.4, -10.0, -12.4, -14.8, -18.6, -24.0, -29.0, -33.0,
    ],
};
pub static U2_ARC: ArcTable = ArcTable {
    arc: &[0.5, 0.6, 0.7, 0.8, 0.9, 1.0],
    pitch: &[-8.0, -19.4, -33.7, -48.3, -57.2, -64.5],
};
pub static U3_ARC: ArcTable = ArcTable {
    arc: &[0.5, 0.6, 0.7, 0.8, 0.9, 1.0],
    pitch: &[-9.5, -20.2, -34.1, -47.6, -59.4, -65.8],
};

impl ArcTable {
    fn slope(&self, i: usize) -> f32 {
        (self.pitch[i + 1] - self.pitch[i]) / (self.arc[i + 1] - self.arc[i])
    }

    pub fn arc_to_pitch(&self, arc: f32) -> f32 {
        let n = self.arc.len();
        let mut i = 0;
        while i + 2 < n && arc > self.arc[i + 1] {
            i += 1;
        }
        self.pitch[i] + (arc - self.arc[i]) * self.slope(i)
    }

    /// The pitch falls as the arc grows.
    pub fn pitch_to_arc(&self, pitch: f32) -> f32 {
        let n = self.arc.len();
        let mut i = 0;
        while i + 2 < n && pitch < self.pitch[i + 1] {
            i += 1;
        }
        self.arc[i] + (pitch - self.pitch[i]) / self.slope(i)
    }
}

/// The aiming camera's response to the right stick: degrees/s at stick values 0.1.. (measured in
/// real aiming with a gun, per game and build). The mouse asks for a rate; the stick that gives
/// it is looked up here.
pub struct StickTable {
    pub x: &'static [f32],
    pub rate: &'static [f32],
}
pub struct AimCurve {
    pub h: StickTable,
    pub v: StickTable,
}

const X12: &[f32] = &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.5, 3.0];
const X10: &[f32] = &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];

pub static U1_100_AIM: AimCurve = AimCurve {
    h: StickTable {
        x: X12,
        rate: &[
            0.0, 1.1, 3.1, 6.0, 10.1, 15.3, 21.6, 29.3, 38.3, 47.8, 74.1, 157.0,
        ],
    },
    v: StickTable {
        x: X12,
        rate: &[
            0.0, 0.5, 1.4, 2.5, 4.3, 6.4, 9.1, 12.3, 16.1, 19.8, 31.0, 39.6,
        ],
    },
};
pub static U1_102_AIM: AimCurve = AimCurve {
    h: StickTable {
        x: X10,
        rate: &[0.0, 3.3, 7.1, 11.4, 16.2, 21.5, 27.0, 33.0, 39.1, 45.0],
    },
    v: StickTable {
        x: X10,
        rate: &[0.0, 1.5, 3.1, 5.1, 7.1, 9.5, 11.9, 14.6, 17.1, 19.9],
    },
};
pub static U23_100_AIM: AimCurve = AimCurve {
    h: StickTable {
        x: X12,
        rate: &[
            0.0, 1.1, 3.0, 5.9, 9.8, 14.8, 21.0, 28.3, 37.0, 46.3, 72.1, 153.9,
        ],
    },
    v: StickTable {
        x: X12,
        rate: &[
            0.0, 0.65, 1.85, 3.65, 6.15, 9.4, 13.4, 18.1, 23.6, 29.5, 46.5, 62.6,
        ],
    },
};
pub static U23_102_AIM: AimCurve = AimCurve {
    h: StickTable {
        x: X10,
        rate: &[0.0, 3.4, 6.8, 11.2, 16.3, 21.5, 26.9, 33.1, 40.5, 46.0],
    },
    v: StickTable {
        x: X10,
        rate: &[0.0, 2.1, 4.5, 7.4, 10.5, 14.0, 17.7, 21.6, 25.8, 30.0],
    },
};

pub fn stick_for_rate(t: &StickTable, rate: f32) -> f32 {
    let n = t.x.len();
    let r = rate.abs();
    let mut x = t.x[n - 1];
    if r < t.rate[n - 1] {
        let mut i = 0;
        while i + 2 < n && r > t.rate[i + 1] {
            i += 1;
        }
        x = t.x[i] + (r - t.rate[i]) / (t.rate[i + 1] - t.rate[i]) * (t.x[i + 1] - t.x[i]);
    }
    if r <= 0.0 {
        x = 0.0;
    }
    if rate < 0.0 {
        -x
    } else {
        x
    }
}

/// The follow camera's response to the right stick value (measured in U1): about 144 x^2
/// degrees/s up to x = 0.5, then a speed that builds up over a second towards 240 degrees/s
/// from x = 0.7 (and stays there above 0.9): fast turns reach it late.
pub fn follow_stick(rate: f32) -> f32 {
    let r = rate.abs();
    let x = if r <= 36.0 {
        (r / 144.0).sqrt()
    } else {
        0.5 + (r - 36.0) / (240.0 - 36.0) * 0.4
    }
    .min(0.9);
    if rate < 0.0 {
        -x
    } else {
        x
    }
}

/// Turns an aim direction (x, y, z) by yaw and pitch (degrees), keeping its length; pitch
/// stays within +-70 degrees. Same yaw sense as the follow camera: a positive yaw is a turn
/// right. False (nothing changed) for a direction of odd length or near vertical.
pub fn aim_turn(v: &mut [f32; 3], yaw: f32, pitch: f32) -> bool {
    let [x, y, z] = *v;
    let len = (x * x + y * y + z * z).sqrt();
    let h = (x * x + z * z).sqrt();
    if !(len > 0.5 && len < 1.5) || h < 1e-4 {
        return false;
    }
    let a = yaw * RADIANS;
    let (s, c) = a.sin_cos();
    let hx = (x * c - z * s) / h;
    let hz = (x * s + z * c) / h;
    let p = (y.atan2(h) + pitch * RADIANS).clamp(-1.2217, 1.2217);
    v[0] = hx * p.cos() * len;
    v[1] = p.sin() * len;
    v[2] = hz * p.cos() * len;
    true
}

fn turn_direction(dir: &mut [f32; 3], degrees: f32) {
    let (s, c) = (degrees * RADIANS).sin_cos();
    let (x, z) = (dir[0], dir[2]);
    dir[0] = x * c - z * s;
    dir[2] = x * s + z * c;
}

#[derive(Clone, Copy, Default)]
struct Pending {
    camera: u64,
    last_us: u64,
    x: f32,
    y: f32,
    used: bool,
}

#[derive(Default)]
struct FrameMotion {
    last_us: u64,
    session_press: u64,
    last_sample: u64,
    batch: (f32, f32),
    pending: [Pending; 4],
}

#[derive(Clone, Copy, Default)]
struct Given {
    controller: u64,
    last_us: u64,
    yaw: f32,
    pitch: f32,
}

#[derive(Default)]
struct AimState {
    other_seen: Vec<u64>,
    first: bool,
    session_press: u64,
    /// The mouse's turns in this aim press (degrees).
    turned_yaw: f32,
    turned_pitch: f32,
    given: [Given; 4],
}

/// Everything the camera callbacks and the pad read share.
pub struct Camera {
    pub input: CameraInput,
    pub knobs: Knobs,
    /// Mouse motion (pixels) not yet given to the follow camera [0] and to the joypad state
    /// [1]: each gets all of it.
    pub dx: [f32; 2],
    pub dy: [f32; 2],
    /// Input sample owning the queued motion.
    pub sample: u64,
    /// Mouse pixels not yet given to the aim direction.
    pub aim_dx: f32,
    pub aim_dy: f32,
    /// Last time the aiming camera's aim controller ran.
    pub aim_direct_us: u64,
    /// When aim was pressed: raw aim is armed only once the controller has run after this time.
    pub aim_press_us: u64,
    /// L2 is held: the aiming camera (also while it blends in).
    pub aim_held: bool,
    /// Last cover camera callback: it turns with the mouse too.
    pub cover_hook_us: u64,
    /// Last follow camera callback: it turns with the mouse.
    pub camera_hook_us: u64,
    /// Last joypad callback: the mouse is a right stick value there.
    pub sticks_hook_us: u64,
    pub follow_direct: bool,
    pub follow_kept: f32,
    /// Degrees the direction is short of the wanted heading.
    pub follow_correction: f32,
    /// The right stick while the direction turns (-0.3, 0, 0.3).
    pub follow_push: f32,
    /// The follow camera's vertical arc is known.
    pub follow_has_arc: bool,
    pub follow_states: [FollowState; 4],
    /// The state the game's pad update fills (its first argument).
    pub joypad_state: u64,
    sticks_last_us: u64,
    push_sign: f32,
    push_us: u64,
    frame: FrameMotion,
    aim: AimState,
}

impl Camera {
    pub fn new(knobs: Knobs) -> Camera {
        Camera {
            input: CameraInput::default(),
            knobs,
            dx: [0.0; 2],
            dy: [0.0; 2],
            sample: 0,
            aim_dx: 0.0,
            aim_dy: 0.0,
            aim_direct_us: 0,
            aim_press_us: 0,
            aim_held: false,
            cover_hook_us: 0,
            camera_hook_us: 0,
            sticks_hook_us: 0,
            follow_direct: true,
            follow_kept: 1.0,
            follow_correction: 0.0,
            follow_push: 0.0,
            follow_has_arc: false,
            follow_states: [FollowState::default(); 4],
            joypad_state: 0,
            sticks_last_us: 0,
            push_sign: 0.0,
            push_us: 0,
            frame: FrameMotion::default(),
            aim: AimState {
                first: true,
                ..AimState::default()
            },
        }
    }

    /// Armed only while the hooked aiming camera's controller has run in THIS press (and
    /// within the last second): an aim that never reaches the hooked caller (a turret, a
    /// vehicle, another caller) keeps the stick path, so the mouse is never dead.
    pub fn raw_aim_armed(&self, now: u64) -> bool {
        self.aim_held
            && self.aim_direct_us != 0
            && self.aim_direct_us >= self.aim_press_us
            && now.wrapping_sub(self.aim_direct_us) < 1_000_000
    }

    /// Takes the queued motion of one consumer and the sample that owns it.
    pub fn take(&mut self, queue: usize) -> (f32, f32, u64) {
        let taken = (self.dx[queue], self.dy[queue], self.sample);
        self.dx[queue] = 0.0;
        self.dy[queue] = 0.0;
        taken
    }

    /// The follow camera's state for `camera`: its own slot, else the least recently used.
    pub fn follow_state(&mut self, camera: u64, now: u64) -> usize {
        let mut slot = 0;
        for i in 0..4 {
            if self.follow_states[i].camera == camera {
                self.follow_states[i].last_us = now;
                return i;
            }
            if self.follow_states[i].last_us < self.follow_states[slot].last_us {
                slot = i;
            }
        }
        self.follow_states[slot] = FollowState {
            camera,
            last_us: now,
            ..FollowState::default()
        };
        slot
    }

    /// Raw mouse: drain new motion on every callback into each live camera's pending share.
    /// Pad reads can happen between cameras, so a new input sample must not discard their
    /// pending motion. Each camera takes and clears only its own share; after 50 ms away its
    /// slot is forgotten. A new camera gets fresh motion, or this sample's latest batch when
    /// joining without fresh input. Only pending motion is kept, never session totals. An idle
    /// gap or a new aim press clears all shares too.
    pub fn frame_motion(&mut self, camera: u64, now: u64) -> (f32, f32) {
        if self.frame.last_us == 0
            || now.wrapping_sub(self.frame.last_us) > 50_000
            || self.frame.session_press != self.aim_press_us
        {
            self.frame.batch = (0.0, 0.0);
            self.frame.pending = [Pending::default(); 4];
            self.frame.session_press = self.aim_press_us;
        }
        self.frame.last_us = now;
        let (fresh_x, fresh_y, sample) = self.take(0);
        let f = &mut self.frame;
        if sample != f.last_sample {
            f.batch = (0.0, 0.0);
            f.last_sample = sample;
        }
        if fresh_x != 0.0 || fresh_y != 0.0 {
            f.batch = (fresh_x, fresh_y);
        }
        let mut slot = 0;
        for i in 0..4 {
            if f.pending[i].used && now.wrapping_sub(f.pending[i].last_us) > 50_000 {
                f.pending[i] = Pending::default();
            }
            if f.pending[i].used {
                f.pending[i].x += fresh_x;
                f.pending[i].y += fresh_y;
            }
            if f.pending[i].last_us < f.pending[slot].last_us {
                slot = i;
            }
        }
        if let Some(i) = (0..4).find(|&i| f.pending[i].used && f.pending[i].camera == camera) {
            slot = i;
        }
        let own = f.pending[slot].used && f.pending[slot].camera == camera;
        let out = if own {
            (f.pending[slot].x, f.pending[slot].y)
        } else {
            f.batch
        };
        f.pending[slot] = Pending {
            camera,
            last_us: now,
            x: 0.0,
            y: 0.0,
            used: true,
        };
        out
    }

    /// The follow camera's collision/position update runs once a frame with the camera object:
    /// its horizontal direction `dir` and the vertical arc position `arc` (the right stick up
    /// lowers it).
    ///
    /// The follow camera keeps the turns of its direction while the right stick is held: with
    /// the stick at rest the game sets the direction itself every few frames and most turns
    /// were lost. So while the mouse turns the direction, the joypad callback holds the stick a
    /// little (0.3, about 13 degrees/s) the same way, and the mouse moves a wanted heading:
    /// each frame the direction turns by the mouse's motion plus what the direction is still
    /// short of it. While the mouse rests, the wanted heading follows the direction. Should the
    /// camera keep no turns at all, the mouse goes to the joypad's right stick, with a small
    /// turn each frame to notice when turns are kept again.
    ///
    /// `raw`: raw mouse is on for this game; `raw_arc_max`: the highest arc the raw mouse moves
    /// the camera to.
    #[allow(clippy::too_many_arguments)]
    pub fn follow_camera(
        &mut self,
        now: u64,
        camera: u64,
        dir: &mut [f32; 3],
        arc: Option<&mut f32>,
        table: &ArcTable,
        raw: bool,
        raw_arc_max: f32,
    ) {
        self.follow_has_arc = arc.is_some();
        if self.camera_hook_us != 0 && now.wrapping_sub(self.camera_hook_us) > 100_000 {
            self.take(0);
            for state in &mut self.follow_states {
                state.moved_us = 0;
            }
        }
        self.camera_hook_us = now;
        let heading = heading_of(dir);
        let slot = if raw {
            self.follow_state(camera, now)
        } else {
            0
        };
        let (dx, dy) = if raw {
            self.frame_motion(camera, now)
        } else {
            let (dx, dy, _) = self.take(0);
            (dx, dy)
        };
        if self.aim_held {
            // The aiming camera takes the mouse.
            self.follow_states[slot].moved_us = 0;
            self.follow_states[slot].pending = false;
            return;
        }
        if raw {
            // The shown camera follows the camera's position with the game's smoothing, a
            // spring (fitted to a recording: critically damped, w about 24/s). It moves the
            // position in a straight line, so a direction far ahead cut through the orbit and
            // swung the camera up over Drake. Below, the direction stays within 60 degrees of
            // this spring; turns faster than about 700 degrees/s follow a few frames later
            // through the wanted heading.
            let st = &mut self.follow_states[slot];
            let mut lag_dt = if st.shown_us != 0 {
                now.wrapping_sub(st.shown_us) as f32 * 1e-6
            } else {
                0.0
            };
            if st.shown_us == 0 || lag_dt > 0.5 {
                st.shown = heading;
                st.shown_v = 0.0;
                lag_dt = 0.0;
            }
            let w = 24.0f32;
            let steps = 1 + (lag_dt / 0.004) as i32;
            for _ in 0..steps {
                let h = lag_dt / steps as f32;
                st.shown_v += (w * w * wrap_degrees(heading - st.shown) - 2.0 * w * st.shown_v) * h;
                st.shown = wrap_degrees(st.shown + st.shown_v * h);
            }
            st.shown_us = now;
        }
        if self.follow_states[slot].pending {
            let st = self.follow_states[slot];
            let kept = (wrap_degrees(heading - st.before) / wrap_degrees(st.after - st.before))
                .clamp(0.0, 1.0);
            // A NaN (no turn at all) stays NaN through `clamp`, as `kept<0 ? 0 : kept>1 ? 1 : kept` did.
            self.follow_kept += 0.2 * (kept - self.follow_kept);
            if self.follow_kept < 0.12 {
                self.follow_direct = false;
            } else if self.follow_kept > 0.3 {
                self.follow_direct = true;
            }
            self.follow_states[slot].pending = false;
        }
        let mut shown = heading; // the shown camera's yaw (debug output)
        if self.knobs.debug {
            shown = match super::host::gpu::camera_yaw() {
                Some((yaw, age)) if age <= 0.25 => yaw,
                _ => heading,
            };
        }
        let turn = if self.knobs.camera_test_rate > 0.0 {
            self.knobs.camera_test_rate
        } else {
            dx * MOUSE_DEGREES_PER_PIXEL * self.input.mouse_sensitivity
        };
        // The wanted heading and how far the direction is short of it (degrees still to turn).
        // The camera rests 0.15 s after the mouse stopped, once the direction has reached the
        // wanted heading (fast turns go beyond what the game keeps in a frame) or at the
        // latest after 0.5 s.
        let st = &mut self.follow_states[slot];
        let mut correction = wrap_degrees(heading - st.target);
        let still = now.wrapping_sub(st.moved_us);
        let resting =
            st.moved_us == 0 || (still > 150_000 && (correction.abs() < 0.5 || still > 500_000));
        if turn != 0.0 {
            st.moved_us = now;
        }
        if resting {
            st.target = heading;
            correction = 0.0;
        }
        if correction > 90.0 || correction < -90.0 {
            // At most a quarter turn behind.
            correction = if correction > 0.0 { 90.0 } else { -90.0 };
            st.target = wrap_degrees(heading - correction);
        }
        st.target = wrap_degrees(st.target - turn); // a positive turn lowers the heading
        if !self.follow_direct {
            // The joypad turns the camera (with the shortfall as extra speed); a small probe
            // turn shows whether direct turns are kept again.
            self.follow_correction = correction;
            self.follow_push = 0.0;
            if turn != 0.0 {
                turn_direction(dir, if turn > 0.0 { 0.3 } else { -0.3 });
                st.before = heading;
                st.after = heading_of(dir);
                st.pending = true;
            }
            if self.knobs.debug {
                note(&format!(
                    "Mouse: follow t={:.4} dx={:.1} heading={:.2} shown={:.2} target={:.2} kept={:.2} joypad",
                    now as f64 * 1e-6,
                    dx,
                    heading,
                    shown,
                    st.target,
                    self.follow_kept
                ));
            }
            return;
        }
        self.follow_correction = 0.0;
        // The shortfall is made up a few degrees a frame (no jump when the game keeps turns again).
        correction = correction.clamp(-4.0, 4.0);
        let mut total = turn + correction;
        if raw {
            let lead = wrap_degrees(heading - total - st.shown); // a positive turn lowers the heading
            if lead > 60.0 {
                total += lead - 60.0;
            } else if lead < -60.0 {
                total += lead + 60.0;
            }
            // The stick is held the way the mouse moves (until 0.1 s after it stops): the
            // stick's own turn is then steady and the correction takes it out evenly.
            if turn != 0.0 {
                self.push_sign = if turn > 0.0 { 0.3 } else { -0.3 };
                self.push_us = now;
            }
            self.follow_push = if self.push_us != 0 && now.wrapping_sub(self.push_us) < 100_000 {
                self.push_sign
            } else {
                0.0
            };
        } else {
            self.follow_push = if total > 0.05 {
                0.3
            } else if total < -0.05 {
                -0.3
            } else {
                0.0
            };
        }
        if self.knobs.debug {
            note(&format!(
                "Mouse: follow t={:.4} dx={:.1} dy={:.1} heading={:.2} shown={:.2} target={:.2} turn={:.2} total={:.2} arc={:.4}",
                now as f64 * 1e-6,
                dx,
                dy,
                heading,
                shown,
                st.target,
                turn,
                total,
                arc.as_deref().copied().unwrap_or(-1.0)
            ));
        }
        if total != 0.0 {
            turn_direction(dir, total);
            if total.abs() >= 0.15 {
                st.before = heading;
                st.after = heading_of(dir);
                st.pending = true;
            }
        }
        if let Some(arc) = arc {
            if dy != 0.0 {
                let signed = if self.input.mouse_invert_y { -dy } else { dy };
                let pitch = table.arc_to_pitch(*arc)
                    - signed * MOUSE_DEGREES_PER_PIXEL * self.input.mouse_sensitivity;
                let v = table.pitch_to_arc(pitch);
                // Raw mouse stops below the top of the arc (U2: 0.75): at the top the camera
                // looks straight down and a small sideways move spins the whole view around
                // Drake. Where the game itself has the camera higher, the mouse only lowers it.
                let top = if !raw {
                    1.0
                } else if *arc > raw_arc_max {
                    *arc
                } else {
                    raw_arc_max
                };
                *arc = if v < 0.0 {
                    0.0
                } else if v > top {
                    top
                } else {
                    v
                };
            }
        }
    }

    /// Raw mouse, cover. The cover camera (CameraControlCover) turns with the right stick in
    /// its own update: its horizontal direction turns around the vertical, and its pitch
    /// (degrees) moves within limits (up to `limits[0]`, down to `-limits[1]`). At the start of
    /// the update the mouse turns both directly, the same amount as the follow camera.
    pub fn cover_camera(
        &mut self,
        now: u64,
        camera: u64,
        dir: &mut [f32; 3],
        pitch: &mut f32,
        limits: Option<[f32; 2]>,
    ) {
        if self.cover_hook_us != 0
            && now.wrapping_sub(self.cover_hook_us) > 100_000
            && !(self.camera_hook_us != 0 && now.wrapping_sub(self.camera_hook_us) < 100_000)
        {
            self.take(0);
        }
        self.cover_hook_us = now;
        self.follow_push = 0.0;
        let (dx, dy) = self.frame_motion(camera, now);
        if self.aim_held {
            return; // the aiming camera takes the mouse
        }
        if self.knobs.debug {
            note(&format!(
                "Mouse: cover t={:.4} dx={:.1} dy={:.1} heading={:.2} pitch={:.2} limits={:.1}..{:.1}",
                now as f64 * 1e-6,
                dx,
                dy,
                heading_of(dir),
                pitch,
                limits.map_or(0.0, |l| -l[1]),
                limits.map_or(0.0, |l| l[0])
            ));
        }
        let turn = dx * MOUSE_DEGREES_PER_PIXEL * self.input.mouse_sensitivity;
        if turn != 0.0 {
            turn_direction(dir, turn);
        }
        if let (true, Some(limits)) = (dy != 0.0, limits) {
            let signed = if self.input.mouse_invert_y { -dy } else { dy };
            let mut v = *pitch - signed * MOUSE_DEGREES_PER_PIXEL * self.input.mouse_sensitivity;
            if v > limits[0] {
                v = limits[0];
            }
            if v < -limits[1] {
                v = -limits[1];
            }
            *pitch = v;
        }
    }

    /// Whether this call of the aim controller comes from the aiming camera's own call site
    /// (`ret` is its return address in the image). Another caller (another build) is left
    /// alone and aiming stays with the stick, as without raw mouse; each is told once.
    pub fn aim_caller(&mut self, game: &str, ret: u64, main_return: u64) -> bool {
        if ret == main_return {
            return true;
        }
        if !self.aim.other_seen.contains(&ret) && self.aim.other_seen.len() < 8 {
            self.aim.other_seen.push(ret);
            note(&format!(
                "Mouse: {game} aim controller from another caller (return {ret:#x}), left alone"
            ));
        }
        false
    }

    /// Raw mouse, aiming (U1 and U2, US 1.02). The aim controller gets the aiming camera's
    /// remembered aim direction as its 4th argument: it turns it by the stick, the camera saves
    /// it back, and the shown direction adds aim assist and recoil on top. Called from the
    /// aiming camera's own call site, the mouse turns that direction itself: 0.15 degrees per
    /// pixel times the sensitivities, at any frame rate. `dir` is only asked for when the call
    /// is the camera's own.
    #[allow(clippy::too_many_arguments)]
    pub fn raw_aim<'a>(
        &mut self,
        now: u64,
        game: &str,
        controller: u64,
        ret: u64,
        main_return: u64,
        dir_address: u64,
        dir: impl FnOnce() -> &'a mut [f32; 3],
    ) {
        if !self.input.mouse_raw || !self.aim_caller(game, ret, main_return) {
            return;
        }
        let dir = dir();
        if self.aim.first {
            self.aim.first = false;
            note(&format!(
                "Mouse: {game} raw aim on: controller {controller:#x}, remembered direction {dir_address:#x} (controller {:+})",
                dir_address.wrapping_sub(controller) as i64
            ));
        }
        let (dx, dy) = (self.aim_dx, self.aim_dy);
        self.aim_dx = 0.0;
        self.aim_dy = 0.0;
        self.aim_direct_us = now;
        let k = MOUSE_DEGREES_PER_PIXEL
            * self.input.mouse_sensitivity
            * self.input.mouse_aim_sensitivity;
        if self.knobs.debug {
            note(&format!(
                "Mouse: raw aim t={:.4} ctl={:#x} yaw={:.3} pitch={:.3} dx={:.1} dy={:.1}",
                now as f64 * 1e-6,
                controller,
                (-dir[0]).atan2(-dir[2]) * 57.29578,
                dir[1].atan2((dir[0] * dir[0] + dir[2] * dir[2]).sqrt()) * 57.29578,
                dx,
                dy
            ));
        }
        // While aim is held the game may swap in new aiming cameras (after a roll: a new one
        // about every 100 ms while Drake gets up), which threw away the turns given to the one
        // before. A new controller in the same aim press gets the mouse's turns of this press
        // on top of the game's own direction. On the same controller the game's own changes
        // (aim assist, recoil) stay. Two aiming cameras can also take turns within one frame
        // (blending): each controller keeps how much of this press's turns it already has, so
        // each gets every turn exactly once.
        let a = &mut self.aim;
        if a.session_press != self.aim_press_us {
            a.session_press = self.aim_press_us;
            a.turned_yaw = 0.0;
            a.turned_pitch = 0.0;
            a.given = [Given::default(); 4];
        }
        if !self.aim_held {
            return;
        }
        let mut slot = 0;
        for i in 0..4 {
            if a.given[i].controller == controller {
                slot = i;
                break;
            }
            if a.given[i].last_us < a.given[slot].last_us {
                slot = i;
            }
        }
        // Controller identity, not direction proximity, determines which turns were already given.
        if a.given[slot].controller != controller {
            a.given[slot] = Given {
                controller,
                ..Given::default()
            };
        }
        a.given[slot].last_us = now;
        if dx != 0.0 || dy != 0.0 {
            a.turned_yaw += dx * k;
            a.turned_pitch += -(if self.input.mouse_invert_y { -dy } else { dy }) * k;
        }
        let yaw = a.turned_yaw - a.given[slot].yaw;
        let pitch = a.turned_pitch - a.given[slot].pitch;
        // A turn the direction rejected (odd length, near vertical) stays owed, not marked as given.
        if (yaw == 0.0 && pitch == 0.0) || aim_turn(dir, yaw, pitch) {
            a.given[slot].yaw = a.turned_yaw;
            a.given[slot].pitch = a.turned_pitch;
        }
    }

    /// The mouse as a right stick value in a float joypad state (lx ly rx ry at +0 and again
    /// at +0x10), called once a frame by the hook of the game's pad update for pad 0. `pad`
    /// holds those 8 floats.
    pub fn mouse_sticks(&mut self, now: u64, pad: &mut [f32], aim: &AimCurve) {
        self.sticks_hook_us = now;
        let mut dt = if self.sticks_last_us != 0 {
            now.wrapping_sub(self.sticks_last_us) as f32 * 1e-6
        } else {
            1.0 / 60.0
        };
        self.sticks_last_us = now;
        dt = dt.clamp(1.0 / 480.0, 0.05);
        if self.knobs.follow_test > 0.0 {
            pad[2] += self.knobs.follow_test;
            pad[6] += self.knobs.follow_test;
            return;
        }
        let (mut dx, mut dy, _) = self.take(1);
        let (x, y);
        if self.knobs.cal_scale > 0.0 {
            x = dx / dt / self.knobs.cal_scale;
            y = dy / dt / self.knobs.cal_scale;
        } else if self.knobs.aim_test > 0.0 {
            x = self.knobs.aim_test;
            y = 0.0;
        } else {
            // The turn rate (degrees/s) the mouse asks for, through the inverse of the aiming
            // curve measured in U1: 45 x^2 degrees/s up to x = 1, then about 54 x - 9.
            let scale = MOUSE_DEGREES_PER_PIXEL * self.input.mouse_sensitivity / dt;
            let following =
                self.camera_hook_us != 0 && now.wrapping_sub(self.camera_hook_us) < 50_000;
            let mut push = 0.0;
            if self.raw_aim_armed(now) {
                // raw mouse: the aim direction turned itself
                dx = 0.0;
                dy = 0.0;
            }
            if !self.aim_held && following && self.follow_direct {
                // the follow camera turned itself
                dx = 0.0;
                dy = 0.0;
                push = self.follow_push;
            }
            if !self.aim_held
                && self.cover_hook_us != 0
                && now.wrapping_sub(self.cover_hook_us) < 50_000
            {
                // raw mouse: the cover camera too
                dx = 0.0;
                dy = 0.0;
                push = 0.0;
            }
            // Following through the joypad: the shortfall is made up within about a quarter second.
            let extra = if !self.aim_held && following && !self.follow_direct {
                self.follow_correction * 4.0
            } else {
                0.0
            };
            let ry = (if self.input.mouse_invert_y { -dy } else { dy }) * scale;
            if self.aim_held {
                x = stick_for_rate(&aim.h, dx * scale);
                y = stick_for_rate(&aim.v, ry);
            } else {
                x = follow_stick(dx * scale + extra) + push;
                y = follow_stick(ry);
            }
        }
        if self.knobs.debug {
            note(&format!(
                "Mouse: aim t={:.4} dt={:.4} dx={:.1} x={:.3} game={:.3}",
                now as f64 * 1e-6,
                dt,
                dx,
                x,
                pad[2]
            ));
        }
        if x != 0.0 {
            pad[2] += x;
            pad[6] += x;
        }
        if y != 0.0 {
            pad[3] += y;
            pad[7] += y;
        }
    }
}
