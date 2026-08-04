from PIL import Image, ImageDraw

def create_icon():
    # Create a 512x512 image with a blue background
    img = Image.new('RGBA', (512, 512), color=(6, 182, 212)) # Cyan-500
    
    # Draw a simple white circle
    d = ImageDraw.Draw(img)
    d.ellipse((128, 128, 384, 384), fill=(255, 255, 255))
    
    # Save as PNG
    img.save('app-icon.png')
    print("Icon generated: app-icon.png")

if __name__ == "__main__":
    create_icon()
