//! 读取全局鼠标位置（屏幕物理像素），鼠标在窗口外也能拿到。
//! 只做这一件事：一个手写的 Win32 GetCursorPos 绑定，不引入 windows-sys。

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetCursorPos(point: *mut Point) -> i32;
}

/// 鼠标在屏幕上的位置（物理像素；多显示器时可以是负数）。
/// winit 已把进程设为按显示器感知 DPI，所以这里和 Window::inner_position 是同一个坐标系。
/// 取不到时（例如锁屏的安全桌面）返回 None。
pub fn screen_position() -> Option<(i32, i32)> {
    let mut point = Point { x: 0, y: 0 };
    // SAFETY: point 是一个有效、可写的 POINT，GetCursorPos 只往里写两个 i32。
    let ok = unsafe { GetCursorPos(&mut point) };
    (ok != 0).then_some((point.x, point.y))
}
