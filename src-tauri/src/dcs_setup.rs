//! Pinned DCS-Skunkworks installation. No game installation files or Lua execute here.
//! A write-ahead journal owns only matching bytes; recovery preserves external edits.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) const VERSION: &str = "0.11.7";
pub(crate) const ARCHIVE_URL: &str =
    "https://github.com/DCS-Skunkworks/dcs-bios/releases/download/v0.11.7/DCS-BIOS_v0.11.7.zip";
pub(crate) const ARCHIVE_SHA256: &str =
    "3272bc07210fa6c560f115cfb5dc8881accf7af5e7a5e218e2c46e1fd89e3292";
const ARCHIVE_SIZE: usize = 8_234_988;
const MAX_UNPACKED: usize = 96 * 1024 * 1024;
const MAX_FILE: usize = 8 * 1024 * 1024;
const MAX_EXPORT: usize = 1024 * 1024;
const LOADER: &str = "dofile(lfs.writedir() .. [[Scripts\\DCS-BIOS\\BIOS.lua]])";
const CONFIG_HASH: &str = "30bd219868020321aa513d1090ae40d656b8fa4ba08d08ae5ae1cd7edadbcc44";
const JOURNAL: &str = "dcs-setup-journal.json";
// Digests derived from the checksum-verified official runtime archive, never executed.
const RUNTIME_HASHES: &[(&str, &str)] = &[
    (
        "BIOS.lua",
        "075b99ae6913f18eef8541eb7ede469026d8bec21322a5dff0c2ce0c81fc5221",
    ),
    (
        "lib/AircraftList.lua",
        "d714b0b975f1435661d775a548e51db2db30320981eb14d4cd48146fd0bbf463",
    ),
    (
        "lib/BIOSStateMachine.lua",
        "84b87f24121b6cfa477b3d3d61bafcfc6da066e472fe14be7e490f0d61cf69c3",
    ),
    (
        "lib/common/Functions.lua",
        "ed63b461a30fca245c0cb03961615ded0aa519211880abb374a1bd6a4e70ebd8",
    ),
    (
        "lib/common/JSONHelper.lua",
        "528690fd49e27ccbd11e29ca7fe2dc1b8ac9ad6617820e0657470c9faf4a284b",
    ),
    (
        "lib/common/Log.lua",
        "0bb93a432aa9ffebf7765e4ebc765380de84b77e00534edb3e504cb57c6840ad",
    ),
    (
        "lib/common/Logger.lua",
        "99f48f73024d0c5faaf8663894d0c2988e9234bd31cbb8224ca2bf1bd74dbbe2",
    ),
    (
        "lib/ConnectionManager.lua",
        "e26df57f1b89fe416ec90d4050d7418ded6b247db7577545abdfca7c2c62977d",
    ),
    (
        "lib/ext/JSON.lua",
        "a471558d5c8fc840616c6f3dc7cc0683f54d79be96e90147ec136fde791955d2",
    ),
    (
        "lib/io/Connection.lua",
        "2417e73e831a2f3a529ef49b0cdea14c385841236475db50b57ff830116a4c37",
    ),
    (
        "lib/io/ReadableConnection.lua",
        "c9b2d8843bdd22f7a0c75aad812c4d3966f5ecb8d0b4eb0fff6d1bef8162deb0",
    ),
    (
        "lib/io/Server.lua",
        "bf69ab706e42ffc6b74c27c838ce7714f1edb25a57000fd10063b7a53e13ad69",
    ),
    (
        "lib/io/Socket_API_defs.lua",
        "8672fb3155e3b272ec91719bb680c1be416079e9be4cf01d87cec9dff3951211",
    ),
    (
        "lib/io/TCPConnection.lua",
        "0182ddbb06d9ddd83fbd3e9589be9d5afc574e5a73a26a6bf113d8d89905b5f9",
    ),
    (
        "lib/io/TCPServer.lua",
        "3f2c792fc6de4a2b87c073c6a667bb66e4d00451b41ee80eaf28d06f41e62dc9",
    ),
    (
        "lib/io/UDPListener.lua",
        "25897a044997f84229c2458c7c452a8db7437f509ea1bdf5572fe47eaba21b84",
    ),
    (
        "lib/io/UDPSender.lua",
        "c330d6fd538e4015a782d95f6c85c89453498858430ae43b53e6feed4af57902",
    ),
    (
        "lib/io/UDPServer.lua",
        "f0b044503dfe84bbb8a7a899b4ebca1778fababcf412e72b09f27b1c37dc34de",
    ),
    (
        "lib/meta_files/DCS_API_defs.lua",
        "672ab21dcadafc671af9ca50a085efc16cd705d3caff174f0f204aed03bb1a5e",
    ),
    (
        "lib/modules/aircraft_modules/A-10C.lua",
        "837534437749eb7e6f8fd94346ae941f2cb369866ae0f3fd5a226cc382ffcf18",
    ),
    (
        "lib/modules/aircraft_modules/A-29B.lua",
        "ed2670f00093d32771fa8738329b18ce269ca21bde013f37c66f637c4a953618",
    ),
    (
        "lib/modules/aircraft_modules/A-4E-C.lua",
        "50918da95fde914ffc853cf896fe8c322fb18f1e7cff93b54eaecba27630497b",
    ),
    (
        "lib/modules/aircraft_modules/AH-64D.lua",
        "139c8607ebb601dcacda582c331f9dc3515f479b70a630fe5af002d28f104770",
    ),
    (
        "lib/modules/aircraft_modules/AH-6J.lua",
        "43d7761187354155a226038c44447a8c7aea9c26367934d503dfa55a672d374d",
    ),
    (
        "lib/modules/aircraft_modules/AJS37.lua",
        "9cd8f2b2f63c47883fc25204c8b0f4c56f18679a2f044b73a90216f34992e85f",
    ),
    (
        "lib/modules/aircraft_modules/Alphajet.lua",
        "0295f5dbd258a895df2ef009e6b6c9c52fe7a8f13c96e9d9f7478460cd76a026",
    ),
    (
        "lib/modules/aircraft_modules/AV8BNA.lua",
        "464196a88e2e7e374a8e419cf5ec03a5873fe936c80448b1bbed7b23b16172da",
    ),
    (
        "lib/modules/aircraft_modules/Bf-109K-4.lua",
        "0ffe97f5409537a8011a617dad566121ff21f706e8f8801bad7dae72d936bd12",
    ),
    (
        "lib/modules/aircraft_modules/C-101.lua",
        "5b9e1ab2b54d8966028189502b3e588aaaf0e7d22802e2ba5f1a2c0979e4890c",
    ),
    (
        "lib/modules/aircraft_modules/C-130J.lua",
        "3dec440586823244478811e2fbfd35322272987c51ff56fbba9705c0592fbc26",
    ),
    (
        "lib/modules/aircraft_modules/CH-47F.lua",
        "9db2c97ef0b878a364743f0661dcb472447bba14262b50a95cf763123c0930cc",
    ),
    (
        "lib/modules/aircraft_modules/Christen Eagle II.lua",
        "aa01d6c775eba3d0e58a07a2ac56db16d38d2fcf44653559f3e81e5c12a4e8d0",
    ),
    (
        "lib/modules/aircraft_modules/Edge540.lua",
        "873821fb5d8009e065f7b5e7d91e38bfd92e1594c56ac6fe5b94b99d5cd82bb1",
    ),
    (
        "lib/modules/aircraft_modules/F-14.lua",
        "8387f1bb6247b9fc9e5a3555407340765022c115f383def75ee0aab76173b44f",
    ),
    (
        "lib/modules/aircraft_modules/F-15E.lua",
        "f31a9387ee9b9bc73695838df1d6dc877e935ef5d8dab6c660b8e1a4527eda6e",
    ),
    (
        "lib/modules/aircraft_modules/F-16C_50.lua",
        "31d8101a04e02e12fb3384cfe1e7f1075b35143cfcebfce0c292d5e729e01da2",
    ),
    (
        "lib/modules/aircraft_modules/F-22A.lua",
        "0fa75b710bd1048e4ee418f80a05f36027fb6c74a298810d0c52f558331edbef",
    ),
    (
        "lib/modules/aircraft_modules/F-4E.lua",
        "1f2d8fd39d031ac127cf66288018678c5f770c80cf6f76f43adbae5d31c511fd",
    ),
    (
        "lib/modules/aircraft_modules/F-5E-3.lua",
        "a589ed1a59b3598fba7f410e2673e204c9fc178d295286abbb585d100494ee97",
    ),
    (
        "lib/modules/aircraft_modules/F-86F Sabre.lua",
        "e26a2aa325be4fa1300d09259826153533e1c575118b231c0cb6db2c552bec80",
    ),
    (
        "lib/modules/aircraft_modules/F4U-1D.lua",
        "d345958b1f4b2e43e9c9aa5d0890ee0d0e7e76f6c9ad5a3f73a11d0b2bf47f9d",
    ),
    (
        "lib/modules/aircraft_modules/FA-18C_hornet.lua",
        "7f5007d68af120499d7f710f2420996a7a332cb848f6b0fcfddc061cbf3218a9",
    ),
    (
        "lib/modules/aircraft_modules/FC3.lua",
        "a21fead2c0c69f122c5a21ac15e0b406b1a1d1e8cc78fa0c535eb608d37c5dd0",
    ),
    (
        "lib/modules/aircraft_modules/FW-190A8.lua",
        "d82c3a5148684530dc894cad56dbeb38a26ce53501716670156edd6e1e151c28",
    ),
    (
        "lib/modules/aircraft_modules/FW-190D9.lua",
        "fd46c528c75a969a7dd5809e9855fbd8ddf6210cedd995862250b24a74cefbf1",
    ),
    (
        "lib/modules/aircraft_modules/I-16.lua",
        "79689f6ce9f21661a1dce6ff163613fec33fe50455a51f50a5cf9950cb9b7118",
    ),
    (
        "lib/modules/aircraft_modules/JF-17.lua",
        "72ed478525b0bb4aea07967fc72e35a0a99adc923fb841d65513716a09aae9b2",
    ),
    (
        "lib/modules/aircraft_modules/Ka-50.lua",
        "a0b545593c1b66b6180e7d427546f964ef55a1a6a177cac00900b419b380d61b",
    ),
    (
        "lib/modules/aircraft_modules/L-39.lua",
        "618d415829efa90fa9333d501bd3af423ddfeed8ea7cef33c0b1d66f697ab7cf",
    ),
    (
        "lib/modules/aircraft_modules/M-2000C.lua",
        "8ab2a3a79142e3bb3976ba0980d53b1296b5559d4ad843da78ae83a8ee9e5b75",
    ),
    (
        "lib/modules/aircraft_modules/MB-339.lua",
        "f924a308a97210a30096707a4edf18b2c01653b9a4a12b5893f224b9e3e778ba",
    ),
    (
        "lib/modules/aircraft_modules/MH-60R.lua",
        "fc1b91a73b07cea4e1435c03eb60b90628ebe84a242295381bfce7db59badd4d",
    ),
    (
        "lib/modules/aircraft_modules/Mi-24P.lua",
        "e25c4116736af65a8d7a1a7d28b2e2f343b7cbc39f2ed0c3848a753f46d6f50b",
    ),
    (
        "lib/modules/aircraft_modules/Mi-8MT.lua",
        "e7e4636d94c693565326bdf7fa4328c0dcb9aabc4d6ac55886e3155325160eb5",
    ),
    (
        "lib/modules/aircraft_modules/MiG-15bis.lua",
        "81e899c76bcdaee542b36dc7290b147df21821c1a9bc36f6123992af038f829d",
    ),
    (
        "lib/modules/aircraft_modules/MiG-19P.lua",
        "6113e19a820f8e1fc281caa086ea9571f207491d36537009a59684df6b0fb4bb",
    ),
    (
        "lib/modules/aircraft_modules/MiG-21Bis.lua",
        "044e21c21f69db8bdabc6572ce49527f3d1a5b33b287aa26e8e4126e9e3378b9",
    ),
    (
        "lib/modules/aircraft_modules/MiG-29A.lua",
        "646b7f7254ea4d4dec2b1659edaf26a99e86cd312e32932adceb12971f20c567",
    ),
    (
        "lib/modules/aircraft_modules/MirageF1.lua",
        "69a907ea87b57783208a74c946acd75e5fcc5ff4c419e7850efd4d6e64bb7431",
    ),
    (
        "lib/modules/aircraft_modules/Mosquito.lua",
        "701cf83e43ffc39261e5bf20136b98624e3de5abbcae5741ad0f8ca8dfde0f50",
    ),
    (
        "lib/modules/aircraft_modules/OH-58D.lua",
        "46ac619911a84912da5cb5f6b3c7b7f9cf31133d6490c2ef611e8f95060e082c",
    ),
    (
        "lib/modules/aircraft_modules/P-47D.lua",
        "a8528f54f3f6921067e885b2d2f18d3a79624c71703d55aab20c88f11c528a5b",
    ),
    (
        "lib/modules/aircraft_modules/P-51D.lua",
        "66b5ee661fafba31ab31a7cade7a640d06e8e7fbbdd760400f6935bef0ad5a33",
    ),
    (
        "lib/modules/aircraft_modules/SA342.lua",
        "55a77678172ec3d5523a464a56ad87cdcaa0a119299b1491983453b51bf2dceb",
    ),
    (
        "lib/modules/aircraft_modules/SpitfireLFMkIX.lua",
        "92c67eb18ebef650491c2564c5c1561570c36fe65ecfe5c6992eb4d48bc6a2fa",
    ),
    (
        "lib/modules/aircraft_modules/UH-1H.lua",
        "7f4ddfb0b79feb8b0d6eec0ede61df20f2c0de2711f816dd4ca333ed504ceab4",
    ),
    (
        "lib/modules/aircraft_modules/VNAO_Room.lua",
        "c0e961864ecef8900ac2ed85209c461179570c702a164e721a0080b9f61a9cfc",
    ),
    (
        "lib/modules/aircraft_modules/VNAO_T-45.lua",
        "d226ac0f4a054d35a0d6a65f15c956f7c115204bd89b75c8e34f7b60b15510dc",
    ),
    (
        "lib/modules/aircraft_modules/Yak-52.lua",
        "0735609170e8b657016fa61e9bde1d446a436ff7e87865a0ef228f622266b1eb",
    ),
    (
        "lib/modules/common_modules/CommonData.lua",
        "2b52d8415a66cf761596465aeb70acaa2c9dbcbfd031e0430f6b2937fba501e1",
    ),
    (
        "lib/modules/common_modules/MetadataEnd.lua",
        "ed99429fa2d65061e380c5ac00bc3fb697a7dc89c5951cef26046598bc5679ce",
    ),
    (
        "lib/modules/common_modules/MetadataStart.lua",
        "57bde79990d053f075e8efcda60eac676779f2de348bede33e7f7b0522816e6b",
    ),
    (
        "lib/modules/common_modules/NS430.lua",
        "7a65ae172789e9204a74a7c933371b0e7c4ed9a5ffc540cf08092a7646ce6054",
    ),
    (
        "lib/modules/common_modules/SuperCarrier.lua",
        "951dea6c0dc6badbd63c0e4f222c05df137bce7be19b84c4015232541c17ffb1",
    ),
    (
        "lib/modules/CommonPositions.lua",
        "cf919b16405adca22dc3a465b5c29fc6ce6617d4cdc4207241742b51268392fd",
    ),
    (
        "lib/modules/ControlDocumentation.lua",
        "ed7c781e9e8217a257992ed69fc0af1439fa18d3b2304382c0b2e008ffac6e6e",
    ),
    (
        "lib/modules/displays/A_10C_CDU.lua",
        "1e82ad200d68c0f42616be05a05fa8b9eaeba0cefc61ed7d7056535a36ca084a",
    ),
    (
        "lib/modules/displays/AH_64D_EUFD.lua",
        "845b159d34d7ec4c262d3546f764e08b59a510b1eb436e5967c2c6580986ec44",
    ),
    (
        "lib/modules/displays/CH_47F_CDU.lua",
        "734b82dfffb54dd5f0d9cbf492fd3a94bff9b8ae371f2b18d45f925021bd0c24",
    ),
    (
        "lib/modules/displays/F_16C_DED.lua",
        "078df33559427191a40098fcb3d93af6a327e544a5ffd07c204e09147f128be5",
    ),
    (
        "lib/modules/documentation/ActionArgument.lua",
        "477ba35f65b56774d7136ca91ca8dd52d4e581b729c1f25624dbb803b6e48cb4",
    ),
    (
        "lib/modules/documentation/ActionInput.lua",
        "83a59476fb80a7f7c29ae5cdaab9232df83a09f9e34d9cf5155a25fa25e6d54f",
    ),
    (
        "lib/modules/documentation/ApiVariant.lua",
        "35e9f1474647f1111b58416a1249629addd6267a03437cc4c0c1756803cbab81",
    ),
    (
        "lib/modules/documentation/Category.lua",
        "c10c6877a29caf2060704d7c1c1f9a6a7a348372fef18dc253647d057c588a99",
    ),
    (
        "lib/modules/documentation/Control.lua",
        "1ce7af2819cd757c3c8e8d28e186bd3733b0255ab6f957ff99a157165cda1351",
    ),
    (
        "lib/modules/documentation/ControlAttributeDocumentation.lua",
        "f1ff5301a46f5a29cacf927cefbc5e497d26a00004d877a5b921312d44b726b4",
    ),
    (
        "lib/modules/documentation/ControlType.lua",
        "70291710cfdac577527d8b0aebcc7a732717d9d566a173552e2d12cc3672ec23",
    ),
    (
        "lib/modules/documentation/DeprecatedAttributeDocumentation.lua",
        "b3b6e01ef0ffb8c5a83955d67831b251b5b34c82ef77d114dee70e5e98bc4618",
    ),
    (
        "lib/modules/documentation/Documentation.lua",
        "b4a982bb6024658a0e16f9a4233974bbdaaee6e34123206084736bb1773b6b3b",
    ),
    (
        "lib/modules/documentation/FixedStepInput.lua",
        "ba18fdfb693d73b2dff1bc1c3d50419aad24eff43db87e42d958e5b41665ae32",
    ),
    (
        "lib/modules/documentation/Input.lua",
        "2403e872fb5ca8e9c51e05856b0cd6a6604a68be4dddd891a69f0bc02107c128",
    ),
    (
        "lib/modules/documentation/InputType.lua",
        "4d7e76a4cd5a7b466e854306948e1a6ec3e7eca20c18272e2cdae974cd47119e",
    ),
    (
        "lib/modules/documentation/IntegerOutput.lua",
        "c9a252db14045ee1dce06a7bf44769dbe80e6a2673ea27e0d21c88ca2b4a1777",
    ),
    (
        "lib/modules/documentation/Output.lua",
        "e03c800032c90ee51c91108622d5e4eda231d026885ac6cdf8e6ee17df2f4f9a",
    ),
    (
        "lib/modules/documentation/OutputType.lua",
        "0a23ed0dccf1db6234960a849d7029ae84af9d1c93e56cc123762a6327489806",
    ),
    (
        "lib/modules/documentation/SetStateInput.lua",
        "03ff9b52bca24b0e45e5ba228791aee62838927a559d06501992a0255e3244a0",
    ),
    (
        "lib/modules/documentation/SetStringInput.lua",
        "1ff2d1435e47d36c9d85f3295e76569e6c8560427b390756fdf8beefe7bb07c8",
    ),
    (
        "lib/modules/documentation/StringOutput.lua",
        "59393e162066aae8ad0369a8b494889885d60b1fda2c35d71178a7922a66b4c2",
    ),
    (
        "lib/modules/documentation/Suffix.lua",
        "b5bb64f33c70db415c57f95f711f070c3ccf768ccd207b8602f4176e3d262343",
    ),
    (
        "lib/modules/documentation/VariableStepInput.lua",
        "364c1e3a457ae200dedea1ef49c6532a183570db174cd5a5e907168e4eee347e",
    ),
    (
        "lib/modules/ICommand.lua",
        "e4984308baa2a1f6c9a788e461852fe73a2c0060c0a10e0b6202be37b721b0fa",
    ),
    (
        "lib/modules/memory_map/MemoryAllocation.lua",
        "9e50672971dffccfade374d02b0e9b1b24d6177459eb5bf087d300d9372520f0",
    ),
    (
        "lib/modules/memory_map/MemoryMap.lua",
        "428208a0299ec818118d616e04ac6b2c087d9d86e661f7630504af8ab56733db",
    ),
    (
        "lib/modules/memory_map/MemoryMapEntry.lua",
        "26aba3aa8673c76f99b7988d3b15653442ecd51e4182e36dd8d92a12c23176ee",
    ),
    (
        "lib/modules/memory_map/StringAllocation.lua",
        "5912d5bd3b109e404782b11fe84985f943f2ed1a78976544995d4ca8eda63ed2",
    ),
    (
        "lib/modules/Module.lua",
        "b8e594a0af5b0693ef58f56709b9a9d0c36eae311992f5984240e95991da31d4",
    ),
    (
        "lib/modules/TextDisplay.lua",
        "cba186cf3c4593190381718517f0e0ef61b52b8fadb260985a67493b72b58867",
    ),
    (
        "lib/Protocol.lua",
        "6496acded1fd0c4876f07afd420db56acc85076ca571bf53b473a564a2df269d",
    ),
];

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    reject_link(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err(format!("{} exceeds the setup file limit.", path.display()));
    }
    Ok(bytes)
}
fn reject_link(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & 0x400 != 0) {
            return Err(format!(
                "{} is a reparse point. Select a regular DCS user folder.",
                path.display()
            ));
        }
    }
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(format!(
            "{} is a link. Select a regular DCS user folder.",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Cannot access {}: {e}", path.display())),
    }
}
fn safe_relative(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() || path.len() > 1024 || path.contains(['\\', ':', '\0']) {
        return Err("Unsafe setup archive path.".into());
    }
    let p = PathBuf::from(path);
    if p.components().any(|c| !matches!(c, Component::Normal(_)))
        || path.split('/').any(|s| {
            s.is_empty()
                || s == "."
                || s == ".."
                || s.contains(['<', '>', '|', '?', '*'])
                || s.ends_with(['.', ' '])
                || s.chars().any(char::is_control)
                || matches!(
                    s.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
    {
        return Err("Unsafe setup archive path.".into());
    }
    Ok(p)
}
fn checked_target(target: &Path) -> Result<PathBuf, String> {
    if !target.is_absolute() || !target.is_dir() {
        return Err("Select an existing absolute DCS user folder in Saved Games. Start DCS once if it is missing.".into());
    }
    let target =
        fs::canonicalize(target).map_err(|e| format!("Cannot access DCS user folder: {e}"))?;
    for p in [
        target.join("Scripts"),
        target.join("Scripts/DCS-BIOS"),
        target.join("Scripts/Export.lua"),
    ] {
        reject_link(&p)?;
    }
    // Selecting the installed game root by mistake must never write there.
    if target.join("bin/DCS.exe").exists() || target.join("bin-mt/DCS.exe").exists() {
        return Err("Choose the DCS Saved Games user folder, not the game installation.".into());
    }
    Ok(target)
}

/// Portable folder discovery; an explicitly supplied custom user folder is selected by the caller.
pub(crate) fn candidates(
    saved_games: &Path,
    install_path: Option<&Path>,
) -> Result<Vec<PathBuf>, String> {
    if !saved_games.is_absolute() {
        return Err("The Saved Games location must be absolute.".into());
    }
    let mut paths = BTreeSet::new();
    for name in ["DCS", "DCS.openbeta"] {
        let p = saved_games.join(name);
        if p.is_dir() {
            paths.insert(p);
        }
    }
    if let Ok(entries) = fs::read_dir(saved_games) {
        for entry in entries.take(128) {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_dir()
                && entry.file_name().to_string_lossy().starts_with("DCS.")
            {
                paths.insert(entry.path());
            }
        }
    }
    if let Some(root) = install_path {
        let variant = root.join("dcs_variant.txt");
        if variant.exists() {
            let raw = read(&variant, 256)?;
            let text = String::from_utf8(raw)
                .map_err(|_| "The DCS user-folder override is not valid UTF-8.")?;
            let suffix = text.trim_start_matches('\u{feff}').trim();
            if suffix.is_empty()
                || suffix.len() > 128
                || suffix.contains(['/', '\\', ':'])
                || suffix.chars().any(char::is_control)
                || suffix == "."
                || suffix == ".."
            {
                return Err(
                    "The DCS user-folder override is ambiguous. Select its user folder explicitly."
                        .into(),
                );
            }
            let selected = saved_games.join(format!("DCS.{suffix}"));
            if selected.is_dir() {
                paths.insert(selected);
            } else {
                return Err(format!("DCS selects {}. Start this installation once or select its user folder explicitly.",selected.display()));
            }
        }
    }
    Ok(paths.into_iter().collect())
}
#[cfg(not(windows))]
pub(crate) fn discover_user_folders(_install_path: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    Err("Automatic Saved Games discovery requires Windows; select a user folder explicitly for fixture verification.".into())
}
#[cfg(windows)]
pub(crate) fn discover_user_folders(install_path: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    #[repr(C)]
    struct Guid {
        a: u32,
        b: u16,
        c: u16,
        d: [u8; 8],
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHGetKnownFolderPath(
            id: *const Guid,
            flags: u32,
            token: *mut std::ffi::c_void,
            path: *mut *mut u16,
        ) -> i32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoTaskMemFree(memory: *mut std::ffi::c_void);
    }
    let id = Guid {
        a: 0x4c5c32ff,
        b: 0xbb9d,
        c: 0x43b0,
        d: [0xb5, 0xb4, 0x2d, 0x72, 0xe5, 0x4e, 0xaa, 0xa4],
    };
    let mut pointer = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(&id, 0, std::ptr::null_mut(), &mut pointer) } < 0
        || pointer.is_null()
    {
        return Err("Windows could not resolve your Saved Games folder. Select the DCS user folder explicitly.".into());
    }
    let mut length = 0;
    while length < 32768 && unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    let folder = String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) });
    unsafe { CoTaskMemFree(pointer.cast()) };
    let folder = folder.map_err(|_| "The Windows Saved Games path is not valid text.")?;
    candidates(Path::new(&folder), install_path)
}

// Analysis text is separate from bytes: append never transcodes existing exports.
#[derive(Clone, Copy)]
enum Encoding {
    Bytes,
    Utf16Le,
    Utf16Be,
}
fn export_text(bytes: &[u8]) -> Result<(String, Encoding), String> {
    let (body, encoding) = if bytes.starts_with(&[0xff, 0xfe]) {
        (&bytes[2..], Encoding::Utf16Le)
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        (&bytes[2..], Encoding::Utf16Be)
    } else {
        (
            bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes),
            Encoding::Bytes,
        )
    };
    let text = match encoding {
        Encoding::Bytes => body.iter().map(|b| char::from(*b)).collect(),
        Encoding::Utf16Le | Encoding::Utf16Be => {
            if body.len() % 2 != 0 {
                return Err("Export.lua has incomplete UTF-16 text.".into());
            }
            let units = body
                .chunks_exact(2)
                .map(|p| {
                    if matches!(encoding, Encoding::Utf16Le) {
                        u16::from_le_bytes([p[0], p[1]])
                    } else {
                        u16::from_be_bytes([p[0], p[1]])
                    }
                })
                .collect::<Vec<_>>();
            String::from_utf16(&units).map_err(|_| "Export.lua has invalid UTF-16 text.")?
        }
    };
    if text.contains('\0') {
        return Err("Export.lua has an unsupported encoding.".into());
    }
    Ok((text, encoding))
}
#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Text(String),
}
fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut result = Vec::new();
    fn long_open(b: &[u8], i: usize) -> Option<(usize, usize)> {
        if b.get(i) != Some(&b'[') {
            return None;
        }
        let mut n = i + 1;
        while b.get(n) == Some(&b'=') {
            n += 1;
        }
        (b.get(n) == Some(&b'[')).then_some((n - i - 1, n + 1))
    }
    fn long_end(b: &[u8], start: usize, equals: usize) -> Option<usize> {
        (start..b.len()).find(|i| {
            b[*i] == b']'
                && b.get(i + 1..i + 1 + equals)
                    .is_some_and(|s| s.iter().all(|c| *c == b'='))
                && b.get(i + 1 + equals) == Some(&b']')
        })
    }
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if bytes.get(index..index + 2) == Some(b"--") {
            index += 2;
            if let Some((n, start)) = long_open(bytes, index) {
                index = long_end(bytes, start, n)
                    .ok_or("Export.lua has an incomplete Lua comment.")?
                    + n
                    + 2;
            } else {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            continue;
        }
        if let Some((n, start)) = long_open(bytes, index) {
            let end =
                long_end(bytes, start, n).ok_or("Export.lua has an incomplete Lua string.")?;
            result.push(Token::Text(
                text[start..end].replace('\\', "/").to_ascii_lowercase(),
            ));
            index = end + n + 2;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"') {
            let quote = bytes[index];
            index += 1;
            let mut value = String::new();
            let mut closed = false;
            while index < bytes.len() {
                let b = bytes[index];
                index += 1;
                if b == quote {
                    closed = true;
                    break;
                }
                if b == b'\\' {
                    let next = *bytes
                        .get(index)
                        .ok_or("Export.lua has an incomplete Lua string.")?;
                    index += 1;
                    value.push(char::from(next));
                } else {
                    value.push(char::from(b));
                }
            }
            if !closed {
                return Err("Export.lua has an incomplete Lua string.".into());
            }
            result.push(Token::Text(value.replace('\\', "/").to_ascii_lowercase()));
            continue;
        }
        let start = index;
        if bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' {
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
        } else if bytes.get(index..index + 2) == Some(b"..") {
            index += 2;
        } else {
            index += 1;
        }
        // ASCII Lua punctuation/identifiers. Advance complete Unicode characters in unknown text.
        while !text.is_char_boundary(index) {
            index += 1;
        }
        result.push(Token::Word(text[start..index].into()));
    }
    Ok(result)
}
fn loader_count(bytes: &[u8]) -> Result<usize, String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return Err("Export.lua is encoded as UTF-16, which DCS cannot load. Its bytes were preserved. Save it as UTF-8 in your editor, then recheck.".into());
    }
    let (text, _) = export_text(bytes)?;
    let tokens = tokens(&text)?;
    let pattern = ["dofile", "(", "lfs", ".", "writedir", "(", ")", ".."];
    let mut canonical = 0;
    let mut references = 0;
    for token in &tokens {
        if let Token::Text(s) = token {
            if s.contains("dcs-bios") || s.ends_with("bios.lua") {
                references += 1;
            }
        }
    }
    for row in tokens.windows(10) {
        if pattern
            .iter()
            .zip(row.iter())
            .all(|(p, t)| matches!(t,Token::Word(w) if w==p))
            && matches!(&row[8],Token::Text(s) if s=="scripts/dcs-bios/bios.lua")
            && row[9] == Token::Word(")".into())
        {
            canonical += 1;
        }
    }
    if references != canonical || canonical > 1 {
        return Err("Export.lua contains duplicate or ambiguous DCS-BIOS loaders. Resolve them before setup; other export scripts were preserved.".into());
    }
    Ok(canonical)
}
fn with_loader(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if loader_count(bytes)? == 1 {
        return Ok(bytes.to_vec());
    }
    let (text, encoding) = export_text(bytes)?;
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let prefix = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        newline
    };
    let suffix = format!("{prefix}-- OpenSimGear DCS-BIOS integration{newline}{LOADER}{newline}");
    let mut output = bytes.to_vec();
    match encoding {
        Encoding::Bytes => output.extend_from_slice(suffix.as_bytes()),
        Encoding::Utf16Le | Encoding::Utf16Be => {
            for c in suffix.encode_utf16() {
                output.extend_from_slice(&if matches!(encoding, Encoding::Utf16Le) {
                    c.to_le_bytes()
                } else {
                    c.to_be_bytes()
                });
            }
        }
    }
    if output.len() > MAX_EXPORT {
        return Err("Export.lua exceeds the setup file limit.".into());
    }
    Ok(output)
}

fn validate_catalogue(path: &Path) -> Result<(), String> {
    let mut count = 0;
    for entry in fs::read_dir(path)
        .map_err(|e| format!("Cannot read installed DCS-BIOS catalogue: {e}"))?
        .take(256)
    {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry
            .path()
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            let value: Value = serde_json::from_slice(&read(&entry.path(), MAX_FILE)?)
                .map_err(|_| format!("{} is not valid catalogue JSON.", entry.path().display()))?;
            if !value.is_object() {
                return Err("The installed DCS-BIOS catalogue must contain JSON objects.".into());
            }
            count += 1;
        }
    }
    if count == 0 {
        return Err("The installed DCS-BIOS catalogue is empty.".into());
    }
    // Validate the generic export metadata and one supported module through the production parser.
    crate::dcs_bios::Catalogue::load_module(path, "A-10C").map(|_| ())
}
fn module_check(module: &Path) -> Result<(), String> {
    reject_link(module)?;
    reject_link(&module.join("doc"))?;
    reject_link(&module.join("doc/json"))?;
    #[cfg(test)]
    if module.join(".osg-authored-fixture").exists() {
        return check_fixture(module);
    }
    for (relative, expected) in RUNTIME_HASHES {
        let path = module.join(relative);
        let mut ancestor = path.parent();
        while let Some(p) = ancestor {
            if p == module {
                break;
            }
            reject_link(p)?;
            ancestor = p.parent();
        }
        if hash(&read(&path, MAX_FILE)?) != *expected {
            return Err(format!("DCS-BIOS {VERSION} runtime file {relative} is changed or incompatible. Existing files were preserved; use a separate clean user folder or restore the official release."));
        }
    }
    let config = read(&module.join("BIOSConfig.lua"), MAX_FILE)?;
    if hash(&config) != CONFIG_HASH {
        return Err(format!("DCS-BIOS version or connection configuration differs from the supported {VERSION} release. Existing settings were preserved; review the module and local connection overrides."));
    }
    validate_catalogue(&module.join("doc/json"))
}
/// Readiness of game-side files only. The caller also validates saved Cockpit connection settings.
pub(crate) fn inspect(target: &Path) -> Result<Value, String> {
    let target = checked_target(target)?;
    let module = target.join("Scripts/DCS-BIOS");
    let present = module.exists();
    let module_error = if present {
        module_check(&module).err()
    } else {
        Some("DCS-BIOS is not installed in this DCS user folder.".into())
    };
    let export = target.join("Scripts/Export.lua");
    let loader = if export.exists() {
        read(&export, MAX_EXPORT).and_then(|b| loader_count(&b))
    } else {
        Ok(0)
    };
    let loader_ready = matches!(loader, Ok(1));
    let mut diagnostics = Vec::<String>::new();
    if let Some(e) = module_error.as_ref() {
        diagnostics.push(e.clone());
    }
    match &loader {
        Ok(0) => diagnostics.push("The DCS-BIOS export loader is missing.".into()),
        Err(e) => diagnostics.push(e.clone()),
        _ => {}
    }
    Ok(
        json!({"userFolder":target,"modulePresent":present,"moduleReady":module_error.is_none(),"loaderReady":loader_ready,"version":if present&&module_error.is_none(){Some(VERSION)}else{None},"supportedVersion":VERSION,"cataloguePath":module.join("doc/json"),"ready":module_error.is_none()&&loader_ready,"repairable":(!present||module_error.is_none())&&loader.is_ok(),"diagnostics":diagnostics}),
    )
}
fn cancelled(check: &impl Fn() -> bool) -> Result<(), String> {
    if check() {
        Err("DCS integration setup was cancelled.".into())
    } else {
        Ok(())
    }
}
fn download(check: &impl Fn() -> bool, progress: &impl Fn(&str, u8)) -> Result<Vec<u8>, String> {
    cancelled(check)?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(2))
        .timeout_write(Duration::from_secs(10))
        .redirects(5)
        .build();
    let response=agent.get(ARCHIVE_URL).set("User-Agent","OpenSimGear-Cockpit-DCS-Setup").call().map_err(|e|format!("Cannot download official DCS-BIOS {VERSION}: {e}. Check your connection and retry."))?;
    if let Some(length) = response.header("Content-Length") {
        if length.parse::<usize>().ok() != Some(ARCHIVE_SIZE) {
            return Err(
                "The DCS-BIOS release archive size differs from the pinned release.".into(),
            );
        }
    }
    let start = Instant::now();
    let mut reader = response.into_reader();
    let mut data = Vec::with_capacity(ARCHIVE_SIZE);
    let mut buffer = [0u8; 32768];
    loop {
        cancelled(check)?;
        if start.elapsed() > Duration::from_secs(120) {
            return Err(
                "The DCS-BIOS download timed out. Retry when the connection is available.".into(),
            );
        }
        let n = reader
            .read(&mut buffer)
            .map_err(|e| format!("The DCS-BIOS download failed: {e}. Retry setup."))?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..n]);
        if data.len() > ARCHIVE_SIZE {
            return Err("The DCS-BIOS archive exceeds the pinned size.".into());
        }
        progress(
            "Downloading official DCS-BIOS",
            (5 + data.len() * 45 / ARCHIVE_SIZE) as u8,
        );
    }
    if data.len() != ARCHIVE_SIZE || hash(&data) != ARCHIVE_SHA256 {
        return Err("The DCS-BIOS release archive failed its pinned SHA-256 check. No game files were changed.".into());
    }
    Ok(data)
}
fn unpack(data: &[u8], check: &impl Fn() -> bool) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data))
        .map_err(|e| format!("Cannot open DCS-BIOS archive: {e}"))?;
    if archive.len() > 2048 {
        return Err("The DCS-BIOS archive has too many entries.".into());
    }
    let mut files = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut size = 0;
    for index in 0..archive.len() {
        cancelled(check)?;
        let entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = entry.name().trim_end_matches('/').to_owned();
        safe_relative(&name)?;
        if entry.is_symlink() {
            return Err("The DCS-BIOS archive contains a link.".into());
        }
        if entry.is_dir() {
            continue;
        }
        let Some(relative) = name.strip_prefix("DCS-BIOS/") else {
            continue;
        };
        let wanted = relative == "BIOS.lua"
            || relative == "BIOSConfig.lua"
            || relative.starts_with("lib/") && relative.ends_with(".lua")
            || relative.starts_with("doc/json/") && relative.ends_with(".json");
        if !wanted {
            continue;
        }
        safe_relative(relative)?;
        if !names.insert(relative.to_ascii_lowercase()) || entry.size() > MAX_FILE as u64 {
            return Err("The DCS-BIOS archive has duplicate or oversized module files.".into());
        }
        let mut bytes = Vec::new();
        entry
            .take(MAX_FILE as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        size += bytes.len();
        if bytes.len() > MAX_FILE || size > MAX_UNPACKED {
            return Err("The DCS-BIOS module exceeds the extraction limit.".into());
        }
        files.insert(relative.to_owned(), bytes);
    }
    if files.len() > 512
        || !files.contains_key("BIOS.lua")
        || !files.contains_key("BIOSConfig.lua")
        || !files.contains_key("doc/json/A-10C.json")
    {
        return Err("The DCS-BIOS archive is missing required runtime/catalogue files.".into());
    }
    Ok(files)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            format!(
                "Cannot create {}: {e}. Check folder permissions.",
                path.display()
            )
        })?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("Cannot save {}: {e}", path.display()))
}
pub(crate) fn replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_file_name(format!(".osg-write-{}", uuid::Uuid::new_v4()));
    if let Err(error) = write_new(&temporary, bytes) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    #[cfg(windows)]
    let result = {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        }
        let a: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 1 | 8) } == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    };
    #[cfg(not(windows))]
    let result = fs::rename(&temporary, path);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Cannot replace {}: {e}", path.display()))
}
fn save_journal(work: &Path, value: &Value) -> Result<(), String> {
    replace(&work.join(JOURNAL), value.to_string().as_bytes())
}
pub(crate) fn recovery_pending(work: &Path) -> bool {
    work.join(JOURNAL).exists()
}
fn journal(work: &Path) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(&read(&work.join(JOURNAL), 2 * 1024 * 1024)?)
        .map_err(|_| "The DCS setup recovery journal is invalid.")?;
    if value["schemaVersion"] != 1
        || value["id"]
            .as_str()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .is_none()
        || !value["files"].is_object()
        || !value["target"]
            .as_str()
            .is_some_and(|s| Path::new(s).is_absolute())
    {
        return Err("The DCS setup recovery journal is invalid.".into());
    }
    for (name, expected) in value["files"].as_object().unwrap() {
        safe_relative(name)?;
        if !expected
            .as_str()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("Invalid DCS setup recovery digest.".into());
        }
    }
    Ok(value)
}
fn clean_owned_tree(root: &Path, files: &Value, conflicts: &mut Vec<String>) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    reject_link(root)?;
    let mut directories = BTreeSet::new();
    for (name, expected) in files
        .as_object()
        .ok_or("Invalid DCS setup file manifest.")?
    {
        if name == ".osg-cockpit-install.json" {
            continue;
        }
        let relative = safe_relative(name)?;
        let path = root.join(relative);
        let mut parent = path.parent();
        while let Some(p) = parent {
            if p == root {
                break;
            }
            reject_link(p)?;
            directories.insert(p.to_path_buf());
            parent = p.parent();
        }
        if path.exists() {
            if hash(&read(&path, MAX_FILE)?) == expected.as_str().unwrap() {
                fs::remove_file(&path)
                    .map_err(|e| format!("Cannot restore {}: {e}", path.display()))?;
            } else {
                conflicts.push(format!(
                    "{} changed after setup; it was preserved.",
                    path.display()
                ));
            }
        }
    }
    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for directory in directories {
        let _ = fs::remove_dir(directory);
    }
    let remaining = fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if remaining
        .iter()
        .all(|entry| entry.file_name() == ".osg-cockpit-install.json")
        && conflicts.is_empty()
    {
        let marker = root.join(".osg-cockpit-install.json");
        if marker.exists() {
            if hash(&read(&marker, MAX_FILE)?)
                == files[".osg-cockpit-install.json"].as_str().unwrap_or("")
            {
                fs::remove_file(&marker).map_err(|e| e.to_string())?;
            } else {
                conflicts.push(format!(
                    "{} changed after setup; it was preserved.",
                    marker.display()
                ));
            }
        }
        if conflicts.is_empty() {
            fs::remove_dir(root).map_err(|e| e.to_string())?;
        }
    } else {
        conflicts.push(format!(
            "{} contains external files or cannot be removed; it was preserved.",
            root.display()
        ));
    }
    Ok(())
}
fn restore(work: &Path, value: &Value) -> Result<Value, String> {
    let target = checked_target(Path::new(
        value["target"].as_str().ok_or("Missing setup target.")?,
    ))?;
    let scripts = target.join("Scripts");
    let id = value["id"].as_str().unwrap();
    let backup = work.join(id).join("Export.lua.before");
    let mut conflicts = Vec::new();
    let export = scripts.join("Export.lua");
    if let Some(after) = value["exportAfter"].as_str() {
        let current = if export.exists() {
            Some(read(&export, MAX_EXPORT)?)
        } else {
            None
        };
        let current_hash = current.as_ref().map(|b| hash(b));
        let before = value["exportBefore"].as_str();
        if current_hash.as_deref() == Some(after) {
            if let Some(before) = before {
                let bytes = read(&backup, MAX_EXPORT)?;
                if hash(&bytes) != before {
                    return Err("The DCS export backup failed its integrity check. Existing files were preserved.".into());
                }
                replace(&export, &bytes)?;
            } else {
                fs::remove_file(&export).map_err(|e| e.to_string())?;
            }
        } else if current_hash.as_deref() != before {
            conflicts.push(
                "Export.lua changed after setup; its current contents were preserved.".into(),
            );
        }
    }
    if value["installModule"] == true && scripts.join("DCS-BIOS").exists() {
        let ownership = scripts.join("DCS-BIOS/.osg-cockpit-install.json");
        if ownership.exists()
            && hash(&read(&ownership, MAX_FILE)?)
                == value["files"][".osg-cockpit-install.json"]
                    .as_str()
                    .unwrap_or("")
        {
            clean_owned_tree(&scripts.join("DCS-BIOS"), &value["files"], &mut conflicts)?;
        } else {
            conflicts.push(
                "The DCS-BIOS folder is not owned by this setup operation; it was preserved."
                    .into(),
            );
        }
    }
    clean_owned_tree(
        &scripts.join(format!(".osg-dcs-stage-{id}")),
        &value["files"],
        &mut conflicts,
    )?;
    if value["createdScripts"] == true {
        let _ = fs::remove_dir(&scripts);
    }
    if conflicts.is_empty() {
        fs::rename(work.join(JOURNAL), work.join(id).join("restored.json"))
            .map_err(|e| e.to_string())?;
    }
    Ok(json!({"restored":conflicts.is_empty(),"conflicts":conflicts,"backupFolder":work.join(id)}))
}
/// Interrupted/failed setup recovery, guarded by matching expected bytes at every affected path.
pub(crate) fn recover(work: &Path) -> Result<Value, String> {
    if !recovery_pending(work) {
        return Ok(json!({"restored":true,"conflicts":[]}));
    }
    restore(work, &journal(work)?)
}
fn install_guarded(
    target: &Path,
    work: &Path,
    files: BTreeMap<String, Vec<u8>>,
    check: &impl Fn() -> bool,
    progress: &impl Fn(&str, u8),
    write_boundary: &impl Fn() -> Result<(), String>,
) -> Result<Value, String> {
    let target = checked_target(target)?;
    let state = inspect(&target)?;
    if state["repairable"] != true {
        return Err(state["diagnostics"][0]
            .as_str()
            .unwrap_or("Existing DCS-BIOS setup needs manual attention.")
            .into());
    }
    let scripts = target.join("Scripts");
    let export = scripts.join("Export.lua");
    let before = if export.exists() {
        Some(read(&export, MAX_EXPORT)?)
    } else {
        None
    };
    let after = with_loader(before.as_deref().unwrap_or(&[]))?;
    let install_module = state["modulePresent"] != true;
    if !install_module && before.as_ref() == Some(&after) {
        progress("DCS-BIOS already configured", 100);
        return Ok(state);
    }
    cancelled(check)?;
    fs::create_dir_all(work).map_err(|e| format!("Cannot create setup recovery folder: {e}"))?;
    if recovery_pending(work) {
        return Err("Recover the interrupted DCS setup before starting again.".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let backup = work.join(&id);
    fs::create_dir(&backup).map_err(|e| e.to_string())?;
    if let Some(bytes) = before.as_ref() {
        write_new(&backup.join("Export.lua.before"), bytes)?;
    }
    let mut files = files;
    if install_module {
        files.insert(
            ".osg-cockpit-install.json".into(),
            json!({"id":id,"version":VERSION,"archiveSha256":ARCHIVE_SHA256})
                .to_string()
                .into_bytes(),
        );
    }
    let manifest: serde_json::Map<String, Value> = files
        .iter()
        .map(|(name, bytes)| (name.clone(), json!(hash(bytes))))
        .collect();
    let value = json!({"schemaVersion":1,"id":id,"target":target,"createdScripts":!scripts.exists(),"installModule":install_module,"files":manifest,"exportBefore":before.as_ref().map(|b|hash(b)),"exportAfter":hash(&after),"version":VERSION,"archiveSha256":ARCHIVE_SHA256});
    save_journal(work, &value)?;
    let result: Result<Value, String> = (|| {
        cancelled(check)?;
        write_boundary()?;
        fs::create_dir_all(&scripts)
            .map_err(|e| format!("Cannot create DCS Scripts folder: {e}. Check permissions."))?;
        if install_module {
            let stage = scripts.join(format!(".osg-dcs-stage-{id}"));
            fs::create_dir(&stage).map_err(|e| e.to_string())?;
            write_new(
                &stage.join(".osg-cockpit-install.json"),
                &files[".osg-cockpit-install.json"],
            )?;
            let total = files.len();
            for (index, (relative, bytes)) in files.iter().enumerate() {
                if relative == ".osg-cockpit-install.json" {
                    continue;
                }
                cancelled(check)?;
                let path = stage.join(safe_relative(relative)?);
                fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
                write_new(&path, bytes)?;
                progress(
                    "Staging DCS-BIOS module",
                    (55 + index * 25 / total.max(1)) as u8,
                );
            }
            module_check(&stage)?;
            cancelled(check)?;
            // Refuse to replace a module created concurrently with setup.
            if scripts.join("DCS-BIOS").exists() {
                return Err("A DCS-BIOS folder appeared during setup. It was preserved; recheck the user folder.".into());
            }
            write_boundary()?;
            fs::rename(&stage, scripts.join("DCS-BIOS"))
                .map_err(|e| format!("Cannot install DCS-BIOS module: {e}"))?;
        }
        cancelled(check)?;
        let current = if export.exists() {
            Some(read(&export, MAX_EXPORT)?)
        } else {
            None
        };
        if current != before {
            return Err(
                "Export.lua changed during setup. Its current contents were preserved.".into(),
            );
        }
        if current.as_ref() != Some(&after) {
            write_boundary()?;
            replace(&export, &after)?;
        }
        progress("Verifying DCS-BIOS module and export loader", 95);
        cancelled(check)?;
        let ready = inspect(&target)?;
        if ready["ready"] != true {
            return Err("DCS-BIOS verification failed. Previous files will be restored.".into());
        }
        fs::rename(work.join(JOURNAL), backup.join("completed.json")).map_err(|e| e.to_string())?;
        progress("DCS-BIOS installed and configured", 100);
        Ok(ready)
    })();
    match result {
        Ok(v) => Ok(v),
        Err(error) => match restore(work, &value) {
            Ok(restored) if restored["restored"] == true => {
                Err(format!("{error} Previous files were restored."))
            }
            Ok(restored) => Err(format!(
                "{error} Recovery preserved external changes: {}",
                restored["conflicts"]
            )),
            Err(recovery) => Err(format!("{error} Recovery is required: {recovery}")),
        },
    }
}
/// The caller serializes this operation, guards active sessions, and supplies Windows capability.
pub(crate) fn setup(
    target: &Path,
    work: &Path,
    check: impl Fn() -> bool,
    progress: impl Fn(&str, u8),
    write_boundary: impl Fn() -> Result<(), String>,
) -> Result<Value, String> {
    if recovery_pending(work) {
        return Err("Recover the interrupted DCS setup before starting again.".into());
    }
    let state = inspect(target)?;
    if state["repairable"] != true {
        return Err(state["diagnostics"][0]
            .as_str()
            .unwrap_or("DCS-BIOS needs attention.")
            .into());
    }
    let files = if state["modulePresent"] != true {
        progress("Downloading official DCS-BIOS", 5);
        unpack(&download(&check, &progress)?, &check)?
    } else {
        BTreeMap::new()
    };
    install_guarded(target, work, files, &check, &progress, &write_boundary)
}

#[cfg(test)]
#[path = "dcs_setup_tests.rs"]
mod tests;
// Authored deterministic fixture; this path does not exist in production builds.
// File bytes and export metadata still pass the same filesystem, loader and catalogue checks.
#[cfg(test)]
fn fixture_files() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::from([
        (
            ".osg-authored-fixture".into(),
            b"OpenSimGear authored DCS setup fixture\n".to_vec(),
        ),
        (
            "BIOS.lua".into(),
            b"-- Authored setup fixture; no native DCS qualification\n".to_vec(),
        ),
        (
            "BIOSConfig.lua".into(),
            b"-- Authored default connection fixture\n".to_vec(),
        ),
    ]);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios");
    for name in [
        "A-10C.json",
        "MetadataStart.json",
        "MetadataEnd.json",
        "AircraftAliases.json",
    ] {
        files.insert(
            format!("doc/json/{name}"),
            fs::read(source.join(name)).unwrap(),
        );
    }
    files
}
#[cfg(test)]
fn check_fixture(module: &Path) -> Result<(), String> {
    for (name, bytes) in fixture_files() {
        if name.starts_with("doc/") {
            continue;
        }
        if read(&module.join(name), MAX_FILE)? != bytes {
            return Err("The authored DCS fixture runtime has changed.".into());
        }
    }
    validate_catalogue(&module.join("doc/json"))
}
#[cfg(test)]
pub(crate) fn fixture_module(target: &Path) {
    for (name, bytes) in fixture_files() {
        let path = target.join("Scripts/DCS-BIOS").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

// Test-only convenience retains the same guarded production install path.
#[cfg(test)]
fn install(
    target: &Path,
    work: &Path,
    files: BTreeMap<String, Vec<u8>>,
    check: &impl Fn() -> bool,
    progress: &impl Fn(&str, u8),
) -> Result<Value, String> {
    install_guarded(target, work, files, check, progress, &|| Ok(()))
}
