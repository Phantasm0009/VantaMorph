# VantaMorph - Advanced Image Morphing Technology

Revolutionary morphing technology that transforms any image into a target image using optimal transport algorithms and physics-based animation.

![example](example.gif)

## ✨ Key Features

- **⚡ Quick Upload Button** - Click and instantly morph any image without configuration
- **🎯 Drag & Drop Support** - Simply drag any image onto the window to transform it
- **📋 Paste from Clipboard** - Press Ctrl+V to paste and morph images directly
- **🚀 Faster Animation** - 6x faster transformation speed for smoother results
- **💡 User-Friendly Interface** - Clear instructions and intuitive controls
- **🎨 Advanced Configuration** - Fine-tune morphing parameters for perfect results

## 🎨 How to Use

### Quick Transform (Instant Mode)
1. Click the **"⚡ quick upload"** button, OR
2. **Drag and drop** any image onto the window, OR
3. **Paste** an image from clipboard (Ctrl+V)
4. Watch as your image automatically morphs into the target!

### Advanced Options
Click **"morph new image"** to access the full configuration UI:
- Change source and target images
- Adjust cropping (tip: for faces, try making the eyes overlap)
- Configure advanced settings:

| Setting               | Description                                                                                     |
|-----------------------|-------------------------------------------------------------------------------------------------|
| resolution            | How many cells the images will be divided into. Higher resolution captures more details. |
| proximity importance  | Controls spatial coherence vs. color matching. Higher values preserve spatial structure. |
| algorithm             | Choose between fast genetic algorithm or optimal (slower but mathematically perfect). |

## 🔬 How It Works

VantaMorph uses state-of-the-art algorithms to create smooth, visually pleasing image transformations:

### 1. Optimal Transport Problem
The morphing process solves an assignment problem using the **Kuhn-Munkres (Hungarian) Algorithm** to find the optimal pixel matching:

```
minimize: Σ c(i,j) * x(i,j)
subject to: Σ x(i,j) = 1 for all i (each source pixel assigned once)
           Σ x(i,j) = 1 for all j (each target pixel receives one)
           x(i,j) ∈ {0,1}
```

### 2. Cost Function
The cost `c(i,j)` between source pixel `i` and target pixel `j` combines color and spatial distances:

```
c(i,j) = w_color * d_color(i,j)² + w_spatial * d_spatial(i,j)²

where:
d_color(i,j) = √[(R_i - R_j)² + (G_i - G_j)² + (B_i - B_j)²]
d_spatial(i,j) = √[(x_i - x_j)² + (y_i - y_j)²]
```

The `proximity_importance` parameter controls the ratio between spatial and color weights:
- Higher values → preserve spatial structure (less dramatic morphing)
- Lower values → optimize color matching (more dramatic transformations)

### 3. Physics-Based Animation
Once the optimal assignment is computed, particles animate from source to destination using a physics simulation:

**Destination Force:**
```
F_dst = k * (p_target - p_current) * |p_target - p_current| * factor(t)

where factor(t) = min((t * dst_force)³, 1000)
```

**Neighbor Repulsion (maintains spacing):**
```
F_neighbor = -Σ w(d) * (p_j - p_i) / d

where w(d) = (personal_space - d) / (d * personal_space) if d < personal_space
```

**Velocity Update (with damping):**
```
v(t+1) = 0.97 * v(t) + a(t)
v(t+1) = clamp(v(t+1), -v_max, v_max)

p(t+1) = p(t) + v(t+1)
```

### 4. GPU-Accelerated Rendering
The app uses **Jump Flooding Algorithm (JFA)** for real-time Voronoi diagram generation:
- Seed texture stores particle positions
- JFA computes nearest-neighbor assignments in O(log n) passes
- Fragment shader colors pixels based on closest particle

**JFA Step Size:**
```
step_k = 2^(ceil(log₂(resolution)) - k)
for k = 0 to ceil(log₂(resolution))
```

## 📦 Installation

### Building from Source

#### Desktop Version (Recommended)
1. Install [Rust](https://www.rust-lang.org/tools/install)
2. Clone this repository
3. Run `cargo run --release` in the project folder
4. The app will open with full drag-and-drop support!

#### Web Version
1. Install [Rust](https://www.rust-lang.org/tools/install)
2. Install the required target: `rustup target add wasm32-unknown-unknown`
3. Install Trunk: `cargo install --locked trunk`
4. Run `trunk serve --release --open --port 3000`
5. Open http://localhost:3000 in your browser

**Note:** Web version supports drag-and-drop and paste, but desktop version provides the best performance.

## 🙏 Attribution & Credits

This project is inspired by and uses code from the original [obamify](https://github.com/Spu7Nix/obamify) by Spu7Nix.

Original project: https://obamify.com/

**What we changed:**
- Renamed to VantaMorph for generalized image morphing
- Added instant drag-and-drop image upload
- Added clipboard paste support (Ctrl+V)
- Implemented quick upload button for one-click transformation
- Increased animation speed (6x faster: 3x per-frame updates, 2.3x force multiplier)
- Enhanced user interface with helpful hints
- Improved user experience for faster workflow

All core transformation algorithms and rendering code are from the original project. This fork focuses on improving the user experience and making image morphing more accessible.

## 🎯 Technical Stack

- **Language:** Rust
- **GUI Framework:** egui (immediate mode GUI)
- **Graphics:** wgpu (WebGPU/WebGL)
- **Algorithm:** Kuhn-Munkres with genetic optimization option
- **Animation:** Custom physics simulation
- **Rendering:** GPU-accelerated JFA-based Voronoi diagrams

## 🤝 Contributing

Please open an issue or a pull request if you have any suggestions or find any bugs!

## 📝 License

Same license as the original obamify project. See LICENSE file for details.

## 🔗 Links

- Original Project: [obamify by Spu7Nix](https://github.com/Spu7Nix/obamify)