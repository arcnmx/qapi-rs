{ config, pkgs, lib, ... }: with pkgs; with lib; let
  inherit (import ./. { pkgs = null; }) checks packages;
in {
  config = {
    name = "qapi-rs";
    ci.gh-actions.enable = true;
    cache.cachix.arc.enable = true;
    channels = {
      nixpkgs = "22.11";
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
      macos.system = "x86_64-darwin";
    };
  };
}
