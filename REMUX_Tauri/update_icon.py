from PIL import Image
import sys
import os

def convert_ico_to_png(ico_path, png_path):
    try:
        img = Image.open(ico_path)
        img.save(png_path, format='PNG')
        print(f"Converted {ico_path} to {png_path}")
    except Exception as e:
        print(f"Error converting icon: {e}")

if __name__ == "__main__":
    convert_ico_to_png(
        r"C:\Users\Maahi\Projects\13. Project_13\logo.ico",
        r"C:\Users\Maahi\Projects\13. Project_13\REMUX_Tauri\src-tauri\icons\icon.png"
    )
    # Also update 32x32 and 128x128 for good measure
    convert_ico_to_png(
        r"C:\Users\Maahi\Projects\13. Project_13\logo.ico",
        r"C:\Users\Maahi\Projects\13. Project_13\REMUX_Tauri\src-tauri\icons\32x32.png"
    )
    convert_ico_to_png(
        r"C:\Users\Maahi\Projects\13. Project_13\logo.ico",
        r"C:\Users\Maahi\Projects\13. Project_13\REMUX_Tauri\src-tauri\icons\128x128.png"
    )
