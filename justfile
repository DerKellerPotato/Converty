name := 'converty'
appid := 'io.github.converty.Converty'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))

bin-src := 'target' / 'release' / name
bin-dst := base-dir / 'bin' / name

desktop := appid + '.desktop'
desktop-src := 'res' / desktop
desktop-dst := base-dir / 'share' / 'applications' / desktop

metainfo := appid + '.metainfo.xml'
metainfo-src := 'res' / metainfo
metainfo-dst := base-dir / 'share' / 'metainfo' / metainfo

icon-svg-src := 'res' / 'icons' / 'hicolor' / 'scalable' / 'apps' / appid + '.svg'
icon-svg-dst := base-dir / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / appid + '.svg'

icon-sym-src := 'res' / 'icons' / 'hicolor' / 'symbolic' / 'apps' / appid + '-symbolic.svg'
icon-sym-dst := base-dir / 'share' / 'icons' / 'hicolor' / 'symbolic' / 'apps' / appid + '-symbolic.svg'

# Default: build the release binary
default: build

# Build the release binary
build *args:
    cargo build --release -p {{name}} {{args}}

# Run the application in release mode
run *args:
    env RUST_BACKTRACE=full cargo run --release -p {{name}} {{args}}

# Lint and test the whole workspace
check:
    cargo clippy --workspace
    cargo test --workspace

# Install system-wide (usually: sudo just install, after just build)
install:
    install -Dm0755 {{bin-src}} {{bin-dst}}
    install -Dm0644 {{desktop-src}} {{desktop-dst}}
    install -Dm0644 {{metainfo-src}} {{metainfo-dst}}
    install -Dm0644 {{icon-svg-src}} {{icon-svg-dst}}
    install -Dm0644 {{icon-sym-src}} {{icon-sym-dst}}

# Remove everything installed by 'install'
uninstall:
    rm -f {{bin-dst}}
    rm -f {{desktop-dst}}
    rm -f {{metainfo-dst}}
    rm -f {{icon-svg-dst}}
    rm -f {{icon-sym-dst}}

# Build and install into ~/.local without sudo
install-local: build
    just prefix="$HOME/.local" install

# Remove the per-user installation from ~/.local
uninstall-local:
    just prefix="$HOME/.local" uninstall

# Build a portable AppImage into dist/ (needs appimagetool, or APPIMAGETOOL=/path)
appimage:
    packaging/appimage/build-appimage.sh
