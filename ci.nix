{ config, channels, pkgs, lib, ... }: with pkgs; with lib; let
  mingwW64-target = channels.rust.lib.targetForConfig.${lib.systems.examples.mingwW64.config};
  rustChannel = channels.rust.stable.override {
    channelOverlays = [
      (cself: csuper: {
        sysroot-std = csuper.sysroot-std ++ [ cself.manifest.targets.${mingwW64-target}.rust-std ];
      })
    ];
  };
  importShell = pkgs.writeText "shell.nix" ''
    import ${builtins.unsafeDiscardStringContext config.shell.drvPath}
  '';
  cargo = name: command: pkgs.ci.command {
    name = "cargo-${name}";
    command = ''
      nix-shell ${importShell} --run ${escapeShellArg ("cargo " + command)}
    '';
    impure = true;
  };
  commas = concatStringsSep ",";
  featureMatrix = rec {
    qmp = singleton "qmp";
    qga = singleton "qga";
    all = qmp ++ qga;
    tokio = all ++ singleton "async-tokio-all";
  };
in {
  config = {
    name = "qapi-rs";
    ci.gh-actions.enable = true;
    cache.cachix.arc.enable = true;
    channels.rust = "master";
    environment.test = {
      inherit (config.rustChannel.buildChannel) cargo;
    };
    tasks = {
      test.inputs = mapAttrsToList (key: features:
        cargo "test-${key}" "test -p qapi --features ${commas features}"
      ) featureMatrix;
      build.inputs = mapAttrsToList (key: features:
        cargo "build-${key}" "build -p qapi --features ${commas features}"
      ) featureMatrix;
      parser.inputs = singleton (cargo "qapi-parser" "test -p qapi-parser");
      spec.inputs = singleton (cargo "qapi-spec" "test -p qapi-spec");
      codegen.inputs = singleton (cargo "qapi-codegen" "test -p qapi-codegen");
      qga.inputs = singleton (cargo "qapi-qga" "test -p qapi-qga");
      qmp.inputs = singleton (cargo "qapi-qmp" "test -p qapi-qmp");
      examples.inputs = singleton (cargo "examples" "build --examples");
    };
    jobs = {
      nixos = {
        tasks.windows.inputs = singleton (cargo "build-windows" "build --examples --target ${mingwW64-target}");
      };
      macos.system = "x86_64-darwin";
    };
  };
  options = {
    rustChannel = mkOption {
      type = types.unspecified;
      default = rustChannel;
    };
    shell = mkOption {
      type = types.unspecified;
      default = config.rustChannel.mkShell {
        nativeBuildInputs = [ git-filter-repo ];
        buildInputs = optional hostPlatform.isDarwin libiconv;
      };
    };
  };
}
