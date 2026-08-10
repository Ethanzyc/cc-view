/// 3x5 点阵数字 0-9（行优先，每行 3 bit，bit=1 为点亮）。
/// 用于在 menu bar 小图标上画 badge 数字。
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111], // 0
    [0b010, 0b110, 0b010, 0b010, 0b111], // 1
    [0b111, 0b001, 0b111, 0b100, 0b111], // 2
    [0b111, 0b001, 0b111, 0b001, 0b111], // 3
    [0b101, 0b101, 0b111, 0b001, 0b001], // 4
    [0b111, 0b100, 0b111, 0b001, 0b111], // 5
    [0b111, 0b100, 0b111, 0b101, 0b111], // 6
    [0b111, 0b001, 0b010, 0b010, 0b010], // 7
    [0b111, 0b101, 0b111, 0b101, 0b111], // 8
    [0b111, 0b101, 0b111, 0b001, 0b111], // 9
];
// "+" 点阵（用于 9+，索引 10）
const PLUS: [u8; 5] = [0b010, 0b010, 0b111, 0b010, 0b010];

/// 在 src 右上角画红圆 + 数字 count。count=0 返回原图副本（无 badge）。
/// count>9 显示 "9+"。纯 RGBA 像素操作，复用 tint_orange 模式，无 image crate。
pub fn draw_badge(src: &tauri::image::Image<'_>, count: usize) -> tauri::image::Image<'static> {
    let w = src.width() as i32;
    let h = src.height() as i32;
    let mut out = src.rgba().to_vec();
    if count == 0 {
        return tauri::image::Image::new_owned(out, w as u32, h as u32);
    }
    let put = |out: &mut [u8], x: i32, y: i32, (r, g, b): (u8, u8, u8)| {
        if x >= 0 && y >= 0 && x < w && y < h {
            let i = (y as usize * w as usize + x as usize) * 4;
            out[i] = r;
            out[i + 1] = g;
            out[i + 2] = b;
            out[i + 3] = 255;
        }
    };
    // badge 圆：右上角，圆心 (w-6, 6)，半径 6
    let cx = w - 6;
    let cy = 6;
    let rad = 6;
    for y in (cy - rad)..=(cy + rad) {
        for x in (cx - rad)..=(cx + rad) {
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= rad * rad {
                put(&mut out, x, y, (255, 69, 58)); // 系统红 #FF453A
            }
        }
    }
    // 数字：count>9 显示 9+（两位：'9' 与 '+'），否则单数字
    let digits: Vec<usize> = if count > 9 {
        vec![9, 10]
    } else {
        vec![count.min(9)]
    };
    // 在圆内居中画点阵（每个数字 3 宽，像素步长 1，居中起点）
    let scale = 1; // menu bar 图标小，1px/点
    let total_w = (digits.len() as i32 * 3 * scale) as i32;
    let start_x = cx - total_w / 2;
    let start_y = cy - 2; // 5 行点阵居中
    for (di, &d) in digits.iter().enumerate() {
        let glyph: [u8; 5] = if d == 10 { PLUS } else { DIGITS[d] };
        let ox = start_x + (di as i32 * 3 * scale);
        for row in 0..5 {
            for col in 0..3 {
                if (glyph[row] >> (2 - col)) & 1 == 1 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            // col/row 是 usize（索引上下文），转 i32 参与坐标算术
                            put(
                                &mut out,
                                ox + col as i32 * scale + sx,
                                start_y + row as i32 * scale + sy,
                                (255, 255, 255),
                            );
                        }
                    }
                }
            }
        }
    }
    tauri::image::Image::new_owned(out, w as u32, h as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_zero_returns_plain() {
        // count=0 不画 badge，返回原图（像素等同）
        let src = tauri::image::Image::new_owned(vec![0u8; 4], 1, 1);
        let out = draw_badge(&src, 0);
        assert_eq!(out.rgba(), src.rgba());
    }

    #[test]
    fn count_positive_draws_red_pixels() {
        // count>0 在右上角画红圆 → 至少一个像素是红 (255,69,58)
        let src = tauri::image::Image::new_owned(vec![0u8; 22 * 22 * 4], 22, 22);
        let out = draw_badge(&src, 3);
        let rgba = out.rgba();
        let red_count = rgba
            .chunks_exact(4)
            .filter(|p| p[0] == 255 && p[1] == 69 && p[2] == 58 && p[3] == 255)
            .count();
        assert!(red_count > 0, "badge must paint red pixels");
    }

    #[test]
    fn count_over_9_capped() {
        // >9 显示 9+：画白点阵（数字 9 与 "+"），不 panic 且有白像素
        let src = tauri::image::Image::new_owned(vec![0u8; 22 * 22 * 4], 22, 22);
        let out = draw_badge(&src, 99);
        let white_count = out
            .rgba()
            .chunks_exact(4)
            .filter(|p| p[0] == 255 && p[1] == 255 && p[2] == 255 && p[3] == 255)
            .count();
        assert!(white_count > 0, "9+ must paint white digit pixels");
    }
}
