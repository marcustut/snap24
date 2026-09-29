{
  description = "Snap 24 — the 24 game: core, Bevy app, and MCP server";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        snap24-mcp = pkgs.rustPlatform.buildRustPackage {
          pname = "snap24-mcp";
          version = "0.1.0";
          src = self;
          cargoLock.lockFile = "${self}/Cargo.lock";
          # This is a mixed workspace — the Bevy app pulls in GPU/audio/X11
          # dependencies we don't want on a server, so build only the MCP server.
          cargoBuildFlags = [ "-p" "snap24-mcp" ];
          cargoInstallFlags = [ "-p" "snap24-mcp" ];
          doCheck = false;
          meta = {
            description = "Snap 24 MCP server (tools + board widget)";
            license = pkgs.lib.licenses.mit;
            mainProgram = "snap24-mcp";
          };
        };
        default = snap24-mcp;
      });

      # Deployable on NixOS: see `services.snap24-mcp` below.
      nixosModules.default = import ./nix/snap24-mcp.nix { inherit self; };
      nixosModules.snap24-mcp = self.nixosModules.default;
    };
}
