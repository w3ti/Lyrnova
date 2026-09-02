Name:           lyrnova
Version:        0.1.0
Release:        0
Summary:        IDE desktop extensível para desenvolvimento de software
License:        GPL-3.0-only
URL:            https://github.com/w3ti/lyrnova
Source0:        %{name}-%{version}.tar.zst
Source1:        %{name}-vendor-%{version}.tar.zst
BuildRequires:  cargo >= 1.85
BuildRequires:  gcc
BuildRequires:  gtk3-devel
BuildRequires:  rust >= 1.85
BuildRequires:  webkit2gtk3-devel
BuildRequires:  zstd
Requires:       bubblewrap
Requires:       git-core
Recommends:     xdg-utils

%description
Lyrnova é um IDE desktop comunitário e extensível. Ele combina editor Monaco,
Explorer, Git e terminal numa aplicação Rust e Tauri. Linguagens, ferramentas
e provedores de IA são capacidades opcionais controladas por plugins.

%prep
%autosetup -p1
tar --zstd -xf %{SOURCE1}

%build
export CARGO_HOME="${PWD}/.cargo-home"
export CARGO_NET_OFFLINE=true
cargo build --release --locked --offline -p lyrnova

%install
install -D -m 0755 target/release/lyrnova %{buildroot}%{_bindir}/lyrnova
install -D -m 0644 packaging/opensuse/io.github.w3ti.lyrnova.desktop \
  %{buildroot}%{_datadir}/applications/io.github.w3ti.lyrnova.desktop
install -D -m 0644 packaging/opensuse/io.github.w3ti.lyrnova.metainfo.xml \
  %{buildroot}%{_datadir}/metainfo/io.github.w3ti.lyrnova.metainfo.xml
for size in 16 32 48 64 128 256 512 1024; do
  install -D -m 0644 "assets/icons/lyrnova-icon-${size}.png" \
    "%{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/io.github.w3ti.lyrnova.png"
done

%check
export CARGO_HOME="${PWD}/.cargo-home"
export CARGO_NET_OFFLINE=true
cargo test --release --locked --offline --workspace

%files
%license LICENSE
%doc README.md CONTRIBUTING.md
%{_bindir}/lyrnova
%{_datadir}/applications/io.github.w3ti.lyrnova.desktop
%{_datadir}/metainfo/io.github.w3ti.lyrnova.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/io.github.w3ti.lyrnova.png

%changelog
