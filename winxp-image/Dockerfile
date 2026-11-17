# Toolchain for the guest software: MinGW-w64 (i686), ImageMagick, cabextract, innoextract and Python for the game
# installs, syslinux for the MBR code
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        gcc-mingw-w64-i686 g++-mingw-w64-i686 binutils-mingw-w64-i686 make imagemagick syslinux-common cabextract innoextract unzip p7zip-full python3 libwin-hivex-perl \
    && rm -rf /var/lib/apt/lists/*
