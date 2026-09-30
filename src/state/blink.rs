//! 眨眼：随机间隔的自动眨眼 + 命令或换眼型触发的眨眼。随机数是固定种子的 xorshift，测试可复现。
//! 不知道眼型；只给出闭合程度，以及“眼睛什么时候完全闭上”。

use super::smoothstep;

/// 一次眨眼的总时长（秒）
const DURATION: f64 = 0.15;
/// 从睁开到完全闭上的时间（秒），之后用剩下的时间慢慢睁开
const CLOSE: f64 = 0.06;
/// 自动眨眼的间隔范围（秒）
const MIN_GAP: f64 = 2.0;
const MAX_GAP: f64 = 6.0;

pub struct Blinker {
    rng: u64,
    /// 正在进行的眨眼的开始时间
    start: Option<f64>,
    /// 排队中的下一次眨眼的开始时间（当前这次已经过了闭眼点时才会排队）
    queued: Option<f64>,
    /// 下一次自动眨眼的时间
    next_random: f64,
}

impl Blinker {
    pub fn new(seed: u64, now: f64) -> Self {
        let mut blinker = Self {
            // xorshift 的状态不能是 0
            rng: seed.max(1),
            start: None,
            queued: None,
            next_random: 0.0,
        };
        blinker.next_random = now + blinker.gap();
        blinker
    }

    /// 安排一次眨眼，返回眼睛完全闭上的时刻。
    /// 没在眨：马上开始。正在闭眼：就用这一次。已经在睁开：接着再眨一次。
    pub fn request(&mut self, now: f64) -> f64 {
        match self.start {
            None => {
                self.start = Some(now);
                now + CLOSE
            }
            Some(start) if now < start + CLOSE => start + CLOSE,
            Some(start) => *self.queued.get_or_insert(start + DURATION) + CLOSE,
        }
    }

    /// 推进到 now，返回闭合程度 0（睁开）..1（闭上）。
    pub fn tick(&mut self, now: f64) -> f32 {
        if let Some(start) = self.start
            && now >= start + DURATION
        {
            self.start = None;
            self.next_random = now + self.gap();
        }
        if self.start.is_none() {
            if let Some(queued) = self.queued
                && now >= queued
            {
                // 排队的从排定的时刻开始算，这样换眼型的时刻和真正闭眼对得上
                self.start = Some(queued);
                self.queued = None;
            } else if now >= self.next_random {
                self.start = Some(now);
            }
        }
        match self.start {
            Some(start) => closedness(now - start),
            None => 0.0,
        }
    }

    pub fn active(&self) -> bool {
        self.start.is_some() || self.queued.is_some()
    }

    /// 空闲时下一次需要醒来的时刻（下一次自动眨眼）。正在眨时返回 None：本来就在逐帧动画。
    pub fn next_wakeup(&self) -> Option<f64> {
        (!self.active()).then_some(self.next_random)
    }

    fn gap(&mut self) -> f64 {
        MIN_GAP + (MAX_GAP - MIN_GAP) * self.next_unit()
    }

    /// xorshift64*：返回 [0, 1) 的均匀随机数
    fn next_unit(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// 眨眼开始后 t 秒的闭合程度：CLOSE 秒内闭上，剩下的时间睁开。
fn closedness(t: f64) -> f32 {
    if t < CLOSE {
        smoothstep((t / CLOSE) as f32)
    } else {
        1.0 - smoothstep(((t - CLOSE) / (DURATION - CLOSE)) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 以 60 fps 跑 seconds 秒，返回每次眨眼开始的时刻
    fn blink_starts(b: &mut Blinker, seconds: f64) -> Vec<f64> {
        let mut starts = Vec::new();
        let mut was_open = true;
        for i in 0..(seconds * 60.0) as usize {
            let now = i as f64 / 60.0;
            let c = b.tick(now);
            if was_open && c > 0.0 {
                starts.push(now);
            }
            was_open = c == 0.0;
        }
        starts
    }

    #[test]
    fn random_blinks_come_every_two_to_six_seconds() {
        let mut b = Blinker::new(42, 0.0);
        let starts = blink_starts(&mut b, 120.0);
        assert!(starts.len() >= 18, "{}", starts.len());
        // 检测到的开始时刻最多比真实开始晚两帧
        assert!((MIN_GAP..=MAX_GAP + 0.04).contains(&starts[0]));
        for pair in starts.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                (MIN_GAP..=MAX_GAP + DURATION + 0.04).contains(&gap),
                "{gap}"
            );
        }
    }

    #[test]
    fn same_seed_gives_same_blinks() {
        let a = blink_starts(&mut Blinker::new(7, 0.0), 60.0);
        let b = blink_starts(&mut Blinker::new(7, 0.0), 60.0);
        let c = blink_starts(&mut Blinker::new(8, 0.0), 60.0);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn requested_blink_closes_then_opens() {
        let mut b = Blinker::new(1, 0.0);
        assert_eq!(b.request(1.0), 1.0 + CLOSE);
        assert_eq!(b.tick(1.0), 0.0);
        assert_eq!(b.tick(1.0 + CLOSE), 1.0);
        assert_eq!(b.tick(1.0 + DURATION), 0.0);
        assert!(!b.active());
        // 眨完之后重新排自动眨眼
        let next = b.next_wakeup().unwrap();
        assert!((1.0 + DURATION + MIN_GAP..=1.0 + DURATION + MAX_GAP).contains(&next));
    }

    #[test]
    fn request_while_closing_reuses_current_blink() {
        let mut b = Blinker::new(1, 0.0);
        b.request(1.0);
        assert_eq!(b.request(1.03), 1.0 + CLOSE);
        assert!(b.queued.is_none());
    }

    #[test]
    fn request_while_opening_queues_a_second_blink() {
        let mut b = Blinker::new(1, 0.0);
        b.request(1.0);
        b.tick(1.1);
        let closed_at = b.request(1.1);
        assert!((closed_at - (1.0 + DURATION + CLOSE)).abs() < 1e-9);
        b.tick(1.0 + DURATION);
        assert!((b.tick(closed_at) - 1.0).abs() < 1e-6);
    }
}
