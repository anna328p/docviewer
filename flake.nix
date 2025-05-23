{
	description = "TODO";

	inputs = {
		# nixpkgs.url = github:nixos/nixpkgs;

		rust-overlay.url = "github:oxalica/rust-overlay";
		rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
	};

	outputs = { self
		, nixpkgs
		, rust-overlay
		, ...
	}@flakes: let
		inherit (nixpkgs.lib) genAttrs systems;

		# forEachSystem' : (Str -> Set Any) -> (Set Any -> Set Any) -> Set (Set Any)
		forEachSystem' = env: body:
			genAttrs systems.flakeExposed (system: body (env system));

		env = system: rec {
			inherit system;

			pkgs = import nixpkgs {
				inherit system;
				overlays = [ rust-overlay.overlays.default ];
			};

			pkgName = "docviewer";

			pkg = pkgs.callPackage ./default.nix { inherit flakes; };
		};

		forEachSystem = forEachSystem' env;

	in {
		packages = forEachSystem (env: {
			${env.pkgName} = env.pkg;
			default = env.pkg;
		});

		devShells = forEachSystem (env: let
			shell = import ./shell.nix { inherit flakes; inherit (env) pkgs pkg; };
		in {
			${env.pkgName} = shell;
			default = shell;
		});
	};
}
