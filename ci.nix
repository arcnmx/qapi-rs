{ pkgs, env, lib, ... }: with pkgs; with lib; let
  inherit (import ./. { pkgs = null; }) checks packages;
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
      nixpkgs = mkIf (env.platform != "impure") "24.05";
    };
    tasks = with checks; {
      test.inputs = [
        test-qapi test-qapi-all
        test-qapi-qmp test-qapi-qga
        test-qapi-async test-qapi-tokio
      ];
      parser.inputs = [ checks.test-parser ];
      spec.inputs = [ checks.test-spec ];
      codegen.inputs = [ checks.test-codegen ];
      qga.inputs = [ checks.test-qga ];
      qmp.inputs = [ checks.test-qmp ];
      examples.inputs = [ packages.examples ];
    };
    jobs = {
      nixos = {
        tasks = {
          windows.inputs = [ packages.examples-windows ];
        };
      };
      macos.system = "aarch64-darwin";
    };
  };
}
