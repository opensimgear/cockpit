# Steam artwork

Steam game rows use existing local artwork first. Once a scan is ready, missing
images are fetched separately in the background using the exact Steam app ID
from a discovered installation source. A disk-only title without a confirmed
Steam identity keeps its placeholder; names are never used to guess a cover.

The native service resolves the current header URL through Steam's public store
metadata, including content-hashed and seasonal assets. It accepts HTTPS image
URLs from Steam's known image hosts for that same app ID, bounds metadata and
image responses, and prepares JPEG previews up to 960 pixels and 128 KiB without
enlarging small originals. For a confirmed `header.jpg`
URL, it also checks the adjacent `header_2x.jpg` and uses it only when its actual
dimensions improve on the original. A failed larger-image request keeps the
metadata-provided header. Hashed seasonal URLs retain their original filenames.

Steam's [store asset documentation](https://partner.steamgames.com/doc/store/assets/standard)
specifies a 920 × 430 header capsule. Local discovery reads both flat cache files
and app-specific content-hash directories, excludes logos and blurred hero
images, and compares the usable pixels after preview resizing and the card's
16:9 crop. A high resolution header can therefore beat a panoramic hero whose
central crop has fewer pixels.

Four background workers share one request per app ID and local data directory.
Prepared images are cached under `steam-artwork/<app-id>.jpg` in the application's
data directory. Subsequent runs can use these images without the network. Older
previews at 384 pixels or smaller are refreshed when a better source is available;
their valid cached image remains the fallback when the network is unavailable. Failed
requests retain placeholders and retry on the next rescan or after a short
backoff. User-selected backgrounds and available local previews are preserved.

The native status response exposes `artworkPending` separately from scan status.
The UI refreshes while downloads are pending, updates individual rows, and stops
the finished review's refresh once artwork succeeds or fails. Downloads never
hold the discovery mutex, change review actions, or delay Apply.

Regression tests live in `games/artwork/steam_online/`. The explicitly ignored
`live_steam_artwork_download_is_cached_and_works_offline` test performs an opt-in
real Steam request and verifies background delivery followed by a cache-only
read. It does not launch a game or change the current library.
