{
  description = "lumo: lightweight Rust Wayland compositor (dev shell)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      runtimeLibs = with pkgs; [
        wayland
        libxkbcommon
        libglvnd
        mesa
        libinput
        seatd
        udev
        libgbm
        libdisplay-info
      ];
    in {
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          rustc cargo clippy rustfmt rust-analyzer
          pkg-config
        ];
        buildInputs = runtimeLibs;
        # Test clients to run inside the nested compositor.
        packages = with pkgs; [ foot ];

        # winit/EGL dlopen these at runtime. /run/opengl-driver/lib is where
        # NixOS puts the real GPU drivers (needs hardware.graphics.enable).
        shellHook = ''
          export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}:/run/opengl-driver/lib:$LD_LIBRARY_PATH"
          export RUST_LOG=''${RUST_LOG:-info}
        '';
      };
    };
}
