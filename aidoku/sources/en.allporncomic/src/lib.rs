#![no_std]

use aidoku::{prelude::*, DeepLinkHandler, ImageRequestProvider, ListingProvider, Source};
use midoku_madara::{Impl, Madara, Params};

struct AllPornComic;

impl Impl for AllPornComic {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: "https://allporncomic.com",
			manga_path: "porncomic",
			popular_order: "views",
			latest_order: "latest",
			ajax_chapters: false,
		}
	}
}

register_source!(
	Madara<AllPornComic>,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);

