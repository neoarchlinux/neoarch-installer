pkgname=neoarch-installer
pkgver=0.1.0
pkgrel=1
pkgdesc="NeoArch Installer"
arch=('x86_64')
url="https://github.com/neoarchlinux/neoarch-installer"
license=('AGPL-3.0')

depends=(
    dialog util-linux bash ncurses coreutils
    gptfdisk parted udev dosfstools btrfs-progs
)

makedepends=(rust cargo clang pkgconf)

source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

binary='neoarch-installer'
target='x86_64-unknown-linux-gnu'

build() {
    cd "$pkgname-$pkgver"
    cargo build --release --locked --target $target
}

package() {
    cd "$pkgname-$pkgver"
    install -Dm755 target/$target/release/$binary "$pkgdir/usr/bin/$binary"
}