#![no_std]

use aidoku::{prelude::*, DeepLinkHandler, ImageRequestProvider, ListingProvider, Source};
use midoku_madara::{Impl, Madara, Params};

struct NovelCrow;

impl Impl for NovelCrow {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: "https://novelcrow.com",
			manga_path: "comic",
			popular_order: "trending",
			latest_order: "latest",
			ajax_chapters: true,
		}
	}
}

register_source!(
	Madara<NovelCrow>,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);

