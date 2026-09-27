import { ContentRating, type Item } from "@suwatte/toolchain/types";

export const itemFrom = (
  id: string,
  title: string,
  coverImage?: string,
  rating: ContentRating = ContentRating.MATURE,
): Item => ({
  id,
  title,
  coverImage,
  rating,
});

export const matureItem = (options: {
  id: string;
  title: string;
  coverImage?: string;
  subtitle?: string;
  webUrl?: string;
}): Item => ({
  ...itemFrom(options.id, options.title, options.coverImage),
  subtitle: options.subtitle,
  webUrl: options.webUrl,
});

export const pageSize = 24;
export const PAGE_SIZE = pageSize;
