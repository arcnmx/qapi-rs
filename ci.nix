{ config, channels, pkgs, env, lib, ... }: with pkgs; with lib; let
  inherit (import ./. { pkgs = null; }) inputs checks packages legacyPackages;
  cargo = name: command: pkgs.ci.command {
    name = "cargo-${name}";
    displayName = "cargo ${command}";
    sourceDep = legacyPackages.source;
    command = "cargo ${command}";
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
    ci.version = "v0.7";
    ci.gh-actions.enable = true;
    cache.cachix = {
      ci.signingKey = "";
      arc.enable = true;
    };
    channels = {
      nixpkgs = mkIf (env.platform != "impure") "23.11";
    };
    environment.test = {
      inherit (inputs.nixpkgs.legacyPackages.${builtins.currentSystem}) cargo;
      inherit (inputs.nixpkgs.legacyPackages.${builtins.currentSystem}.stdenv) cc;
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
