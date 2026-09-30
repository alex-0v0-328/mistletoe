//! 纯逻辑状态：表情参数、动作、鼠标跟随和过渡插值。
//! 不碰 GPU、不做 I/O、不读时钟：时间 `now`（秒，从程序启动算起）由调用方传入，方便单元测试。
//! 第 1 步只有鼠标跟随；表情、眨眼、动作和标签在第 2 步加入。

mod follow;

use follow::Follow;

/// 指针相对窗口中心的位置，以半个窗口短边为单位：x 向右、y 向上。
/// 指针在窗口外时可以超出 ±1，没有上限。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    pub x: f32,
    pub y: f32,
}

/// 球的朝向（弧度）。yaw 为正 = 脸向右转，pitch 为正 = 抬头。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub yaw: f32,
    pub pitch: f32,
}

/// 每帧交给 render 的只读结果。
#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub pose: Pose,
    /// 还在动：app 需要继续请求下一帧；为 false 时 app 可以睡眠。
    pub animating: bool,
}

#[derive(Default)]
pub struct State {
    follow: Follow,
}

impl State {
    /// 更新指针位置。返回 true 表示跟随目标变了，需要重绘。
    pub fn set_pointer(&mut self, pointer: Pointer) -> bool {
        self.follow.set_pointer(pointer)
    }

    /// 推进到时间 `now`，返回这一帧的快照。
    pub fn tick(&mut self, now: f64) -> Snapshot {
        let pose = self.follow.tick(now);
        Snapshot {
            pose,
            animating: !self.follow.settled(),
        }
    }
}
