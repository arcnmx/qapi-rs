{ config, channels, pkgs, lib, ... }: with pkgs; with lib; let
  inherit (import ./. { pkgs = null; }) checks packages devShells;
  importShell = pkgs.writeText "shell.nix" ''
    import ${builtins.unsafeDiscardStringContext devShells.default.drvPath}
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
    async = all ++ [ "async-tower" ];
    tokio = all ++ singleton "async-tokio-all";
  };
in {
  config = {
    name = "qapi-rs";
    ci.gh-actions.enable = true;
    cache.cachix.arc.enable = true;
    channels = {
      nixpkgs = "22.11";
    };
    tasks = with checks; {
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
      examples.inputs = singleton (cargo "examples" "build --examples --bins");
    };
    jobs = {
      nixos = {
        tasks = {
          windows.inputs = [ packages.examples-windows ];
        };
      };
      # XXX: macos.system = "x86_64-darwin";
    };
  };
}
