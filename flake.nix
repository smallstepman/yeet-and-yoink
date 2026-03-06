{
  description = "yeet-and-yoink package and home-manager module";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];

      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));

      mkPackage = pkgs: pkgs.rustPlatform.buildRustPackage {
        pname = "yeet-and-yoink";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        meta = with pkgs.lib; {
          description = "Deep focus/move integration between niri and apps";
          mainProgram = "yeet-and-yoink";
          platforms = platforms.all;
        };
      };

      hmModule = { config, lib, pkgs, ... }:
        let
          cfg = config.programs.yeet-and-yoink;
          tomlFormat = pkgs.formats.toml {};
          generatedConfig = tomlFormat.generate "yeet-and-yoink-config.toml" (lib.filterAttrs (name: _: name != "raw") cfg.config);
          configSource =
            if cfg.config.raw != null
            then pkgs.writeText "yeet-and-yoink-config.toml" cfg.config.raw
            else generatedConfig;
        in {
          options.programs.yeet-and-yoink = {
            enable = lib.mkEnableOption "yeet-and-yoink";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.system}.default;
              defaultText = lib.literalExpression "inputs.yeet-and-yoink.packages.${pkgs.system}.default";
              description = "yeet-and-yoink package to install.";
            };

            config = lib.mkOption {
              type = lib.types.submodule ({ ... }: {
                freeformType = tomlFormat.type;
                options.raw = lib.mkOption {
                  type = lib.types.nullOr lib.types.lines;
                  default = null;
                  description = ''
                    Raw TOML for yeet-and-yoink written as-is. When non-null, this value
                    overrides all other programs.yeet-and-yoink.config.* fields.
                  '';
                };
              });
              default = {};
              description = "yeet-and-yoink runtime configuration.";
            };
          };

          config = lib.mkIf cfg.enable {
            home.packages = [ cfg.package ];
            xdg.configFile."yeet-and-yoink/config.toml".source = configSource;
          };
        };
    in {
      packages = forAllSystems (pkgs:
        rec {
          yeet-and-yoink = mkPackage pkgs;
          default = yeet-and-yoink;
        });

      overlays.default = final: prev: {
        yeet-and-yoink = self.packages.${prev.system}.default;
      };

      homeManagerModules.default = hmModule;
    };
}
