{ pkgs ? import <nixpkgs> { }, ci ? import <ci> { inherit pkgs; } }: ci.config.shell
