{
  description = "QEMU QMP and Guest Agent API";
  inputs = {
    flakelib.url = "github:flakelib/fl";
    nixpkgs = { };
    schema = {
      flake = false;
      type = "github";
      owner = "arcnmx";
      repo = "qemu-qapi-filtered";
      ref = "v10.0.3"; # keep in sync with schema submodule
    };
    rust = {
      url = "github:arcnmx/nixexprs-rust";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs = { self, schema, flakelib, nixpkgs, rust, ... }@inputs: let
    featureMatrix = rec {
      qmp = [ "qmp" ];
      qga = [ "qga" ];
      all = qmp ++ qga;
      async = all ++ [ "async-tower" ];
      tokio = all ++ [ "async-tokio-all" ];
    };
    nixlib = nixpkgs.lib;
    inherit (self.lib) crate;
    libs = nixlib.filterAttrs (_: crate: crate.package.publish or true) crate.members;
    testCrate = package: { buildFeatures ? [ ], ... }@args: with nixlib; let
      crate = self.lib.crate.members.${package};
      flags = [ "-p" crate.name ];
    in { rustPlatform, source }: rustPlatform.buildRustPackage (args // {
      pname = crate.name;
      inherit (crate) cargoLock version;
      src = source;
      cargoTestFlags = flags ++ args.cargoTestFlags or [ ];
      cargoBuildFlags = flags ++ args.cargoBuildFlags or [ ];
      buildType = "debug";
      meta = {
        name = let
          features = " --features ${concatStringsSep "," buildFeatures}";
          cmd = if args.doCheck or true then "test" else "build";
        in "cargo ${cmd} -p ${crate.name}" + optionalString (buildFeatures != [ ]) features;
      } // args.meta or { };
      auditable = false;
      passthru.ci = {
        cache.inputs = [ (rustPlatform.importCargoLock crate.cargoLock) ];
      };
    });
  in flakelib {
    inherit inputs;
    systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    devShells = {
      plain = {
        mkShell, writeShellScriptBin
      , enableRust ? true, cargo
      , rustTools ? [ ]
      , generate
      }: mkShell {
        inherit rustTools;
        nativeBuildInputs = nixlib.optional enableRust cargo
          ++ [
            (writeShellScriptBin "generate" ''nix run .#generate "$@"'')
          ];
      };
      stable = { rust'stable, outputs'devShells'plain }: outputs'devShells'plain.override {
        inherit (rust'stable) mkShell;
        enableRust = false;
      };
      dev = { rust'unstable, rust-w64-overlay, outputs'devShells'plain }: let
        channel = rust'unstable.override {
          channelOverlays = [ rust-w64-overlay ];
        };
      in outputs'devShells'plain.override {
        inherit (channel) mkShell;
        enableRust = false;
        rustTools = [ "rust-analyzer" ];
      };
      default = { outputs'devShells }: outputs'devShells.plain;
    };
    packages = {
      examples = testCrate "examples" {
        doCheck = false;
      };
      examples-windows = { rust-w64, examples }: (examples.override {
        inherit (rust-w64.latest) rustPlatform;
      }).overrideAttrs (old: {
        meta = old.meta // {
          name = "cargo build --target ${rust-w64.latest.hostTarget.triple} -p qapi-examples";
        };
      });
      default = { examples }: examples;
    };
    legacyPackages = {
      # manual src fixup for submodule symlinks
      source = { runCommand }: runCommand crate.src.name {
        preferLocalBuild = true;
        inherit (crate) src;
        inherit (crate.src) pname version;
        inherit schema;
        readme = ./README.md;
      } ''
        mkdir $out
        cp --no-preserve=mode -rs $src/* $out/
        for api in qmp qga; do
          cp -frd $src/$api/schema/* $out/$api/schema/
        done
        ln -s $schema $out/schema
        ln -s $readme $out/README.md
      '';

      rust-w64 = { pkgsCross'mingwW64 }: import inputs.rust { inherit (pkgsCross'mingwW64) pkgs; };
      rust-w64-overlay = { rust-w64 }: let
        target = rust-w64.lib.rustTargetEnvironment {
          inherit (rust-w64) pkgs;
          rustcFlags = [ "-L native=${rust-w64.pkgs.windows.pthreads}/lib" ];
        };
      in cself: csuper: {
        sysroot-std = csuper.sysroot-std ++ [ cself.manifest.targets.${target.triple}.rust-std ];
        cargo-cc = csuper.cargo-cc // cself.context.rlib.cargoEnv {
          inherit target;
        };
        rustc-cc = csuper.rustc-cc // cself.context.rlib.rustcCcEnv {
          inherit target;
        };
      };

      generate = { rust'builders, outputHashes }: rust'builders.generateFiles {
        paths = {
          "lock.nix" = outputHashes;
        };
      };
      outputHashes = { rust'builders }: rust'builders.cargoOutputHashes {
        inherit crate;
      };
    };
    checks = with nixlib; {
      versions = { rust'builders, source }: rust'builders.check-contents {
        src = source;
        patterns = [
          { path = "README.md";
            plain = ''version = "${versions.majorMinor crate.members.qapi.version}"'';
          }
        ] ++ mapAttrsToList (dir: crate: {
          path = "${dir}/src/lib.rs";
          docs'rs = { inherit (crate) name version; };
        }) libs;
      };
    } // mapAttrs' (dir: crate: nameValuePair "test-${dir}" (testCrate dir { })) libs
    // mapAttrs' (name: buildFeatures: nameValuePair "test-qapi-${name}" (testCrate "qapi" {
      inherit buildFeatures;
    })) featureMatrix;
    lib = {
      crate = rust.lib.importCargo {
        inherit self;
        path = ./Cargo.toml;
        inherit (import ./lock.nix) outputHashes;
      };
      inherit (crate.package) version;
    };
    config = rec {
      name = "qapi-rs";
      packages.namespace = [ name ];
    };
  };
}
