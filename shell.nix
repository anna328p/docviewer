{ flakes ? { }
, pkgs ? import <nixpkgs> { }
, specialArgs ? { }
, pkg ? pkgs.callPackage ./. ({ inherit flakes; } // specialArgs)
}:

(pkg.override { }).overrideAttrs (attrs: { })
