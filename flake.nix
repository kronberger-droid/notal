{
  description = "Lean Obsidian vault MCP server for Claude Code";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = {
    self,
    nixpkgs,
  }: let
    forAllSystems = nixpkgs.lib.genAttrs ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
  in {
    packages = forAllSystems (system: let
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      notal = pkgs.rustPlatform.buildRustPackage {
        pname = "notal";
        version = "0.1.0";
        src = ./.;
        cargoHash = "sha256-W0u6Qfn22/72JqwsUdEWo6gvivL2g0Fv+pT0jpOJ5Yc=";
      };
      default = self.packages.${system}.notal;
    });
  };
}
