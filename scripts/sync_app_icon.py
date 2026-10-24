"""从新版图标源图（assets/icon-source.jpg）生成全套品牌图标。

「四处一个图标」的原则不变：标题栏、任务栏、托盘、侧边栏 logo
都用 tray/resources/app_icon.ico；网页端 favicon、登录页与工资页 logo
用 src-tauri/public/app_icon.png。两份产物都从同一张源图生成，
保证所有入口看到的是同一个标识。

处理步骤：
1. 源图是 JPG，四角是白底——从四个角把白色区域洪水填充成透明，
   否则圆角外的白底在深色任务栏上会露出一圈白；
2. 透明边缘先收缩再羽化，去掉 JPG 压缩在图标边沿留下的白晕；
3. 用处理后的图生成多尺寸 .ico（Windows 用）与 256px .png（网页用）。

用法：
    python scripts/sync_app_icon.py
"""
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets" / "icon-source.jpg"
ICO_TARGET = ROOT / "tray" / "resources" / "app_icon.ico"
PNG_TARGET = ROOT / "src-tauri" / "public" / "app_icon.png"

# 洪水填充用的魔法色：源图里不存在，填完后按它抠透明
MAGIC = (255, 0, 255)
# 与种子色的通道差之和小于该值的相邻像素都算背景
FILL_THRESH = 30
ICO_SIZES = [(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)]
PNG_SIZE = (256, 256)


def cut_background(image: Image.Image) -> Image.Image:
    """把四角相连的白色背景抠成透明，并羽化边缘。"""
    rgb = image.convert("RGB")
    # 图标贴着四条边的中点，白色背景只出现在四个角，各填一次
    corners = [(0, 0), (rgb.width - 1, 0), (0, rgb.height - 1), (rgb.width - 1, rgb.height - 1)]
    for corner in corners:
        ImageDraw.floodfill(rgb, corner, MAGIC, thresh=FILL_THRESH)

    # 与魔法色几乎一致的像素 => 透明
    diff = ImageChops.difference(rgb, Image.new("RGB", rgb.size, MAGIC))
    mask = diff.convert("L").point(lambda v: 0 if v < 8 else 255, mode="L")

    rgba = rgb.convert("RGBA")
    # 边缘收缩 1px 再羽化，吃掉紧贴图标的 JPG 白晕
    alpha = mask.filter(ImageFilter.MinFilter(3)).filter(ImageFilter.GaussianBlur(1.2))
    rgba.putalpha(alpha)

    box = rgba.getbbox()
    if box is None:
        raise SystemExit("抠完背景后图标是空的，检查阈值设置")
    return rgba.crop(box)


def pad_to_square(image: Image.Image) -> Image.Image:
    """补成内容居中的正方形，ico 各尺寸按整图缩放。"""
    side = max(image.width, image.height)
    canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    canvas.paste(
        image,
        ((side - image.width) // 2, (side - image.height) // 2),
        image,
    )
    return canvas


def main() -> None:
    if not SOURCE.is_file():
        raise SystemExit(f"找不到图标源图：{SOURCE}")

    icon = pad_to_square(cut_background(Image.open(SOURCE)))

    ICO_TARGET.parent.mkdir(parents=True, exist_ok=True)
    PNG_TARGET.parent.mkdir(parents=True, exist_ok=True)
    icon.save(ICO_TARGET, format="ICO", sizes=ICO_SIZES)
    icon.resize(PNG_SIZE, Image.LANCZOS).save(PNG_TARGET, format="PNG")
    print(f"已生成：{ICO_TARGET.relative_to(ROOT)}（{'/'.join(str(s[0]) for s in ICO_SIZES)}）")
    print(f"已生成：{PNG_TARGET.relative_to(ROOT)}（{PNG_SIZE[0]}px，网页 favicon 与 logo）")


if __name__ == "__main__":
    main()
