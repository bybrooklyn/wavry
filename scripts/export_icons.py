from AppKit import NSImage, NSImageSymbolConfiguration, NSMakeRect, NSImageView
import json
import os
import re

def get_svg_path(symbol_name):
    config = NSImageSymbolConfiguration.configurationWithPointSize_weight_(24, 400)
    image = NSImage.imageWithSystemSymbolName_accessibilityDescription_(symbol_name, None)
    if not image:
        return None
    
    image = image.imageWithSymbolConfiguration_(config)
    rect = NSMakeRect(0, 0, 24, 24)
    
    # Use NSImageView to get PDF data
    view = NSImageView.alloc().initWithFrame_(rect)
    view.setImage_(image)
    
    pdf_data = view.dataWithPDFInsideRect_(rect)
    pdf_text = pdf_data.bytes().tobytes().decode('ascii', errors='ignore')
    
    # Tokenize PDF commands
    tokens = re.split(r'\s+', pdf_text)
    svg_path_parts = []
    buffer = []
    
    for token in tokens:
        if token in ['m', 'l', 'c', 'h']:
            if token == 'm' and len(buffer) >= 2:
                y = float(buffer[-1]); x = float(buffer[-2])
                svg_path_parts.append(f"M {x} {24 - y}")
            elif token == 'l' and len(buffer) >= 2:
                y = float(buffer[-1]); x = float(buffer[-2])
                svg_path_parts.append(f"L {x} {24 - y}")
            elif token == 'c' and len(buffer) >= 6:
                y3 = float(buffer[-1]); x3 = float(buffer[-2])
                y2 = float(buffer[-3]); x2 = float(buffer[-4])
                y1 = float(buffer[-5]); x1 = float(buffer[-6])
                svg_path_parts.append(f"C {x1} {24-y1} {x2} {24-y2} {x3} {24-y3}")
            elif token == 'h':
                svg_path_parts.append("Z")
            buffer = []
        else:
            try:
                # Basic check if it looks like a number
                if re.match(r'^-?\d+(\.\d+)?$', token):
                    buffer.append(token)
                else:
                    buffer = []
            except Exception:
                buffer = []
                
    return " ".join(svg_path_parts) if svg_path_parts else None

# Load manifest
manifest_path = 'design/icons.json'
with open(manifest_path, 'r') as f:
    manifest = json.load(f)

output_dir = 'crates/wavry-desktop/src/assets/icons'
os.makedirs(output_dir, exist_ok=True)

symbols = manifest['icons']
for semantic_name, symbol_name in symbols.items():
    print(f"Exporting {semantic_name} ({symbol_name})...")
    path_data = get_svg_path(symbol_name)
    if path_data:
        svg_content = f'<svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="{path_data}" fill="currentColor"/></svg>'
        with open(os.path.join(output_dir, f"{semantic_name}.svg"), 'w') as f:
            f.write(svg_content)
    else:
        print(f"Failed to export {symbol_name}")

print("Done!")
