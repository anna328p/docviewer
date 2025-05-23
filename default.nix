{ flakes
, stdenv
, lib
, rust-bin
, gtk4
, pkg-config
, pango
, webkitgtk_6_0
, libadwaita
}:

let
	# rust-toolchain = rust-bin.selectLatestNightlyWith (toolchain: toolchain.default);
	rust-toolchain = rust-bin.stable.latest.default;

in stdenv.mkDerivation {
	pname = "docviewer";
	version = "0";

	nativeBuildInputs = [
		pkg-config
	];

	buildInputs = [
		rust-toolchain
		gtk4
		pango
		webkitgtk_6_0
		libadwaita
	];

	meta = {
		maintainers = [ lib.maintainers.anna328p ];
	};
}
