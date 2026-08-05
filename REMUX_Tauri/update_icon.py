from pathlib import Path

from PIL import Image


def convert_ico_to_png(ico_path: Path, png_path: Path) -> None:
    try:
        img = Image.open(ico_path)
        png_path.parent.mkdir(parents=True, exist_ok=True)
        img.save(png_path, format="PNG")
        print(f"Converted {ico_path} to {png_path}")
    except Exception as e:
        print(f"Error converting icon: {e}")


if __name__ == "__main__":
    repo_root = Path(__file__).resolve().parent.parent
    icons_dir = Path(__file__).resolve().parent / "src-tauri" / "icons"
    ico_path = repo_root / "logo.ico"

    for name in ("icon.png", "32x32.png", "128x128.png"):
        convert_ico_to_png(ico_path, icons_dir / name)
