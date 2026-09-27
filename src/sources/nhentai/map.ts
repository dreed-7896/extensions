import {
  ContentRating,
  ContentStatus,
  ContentType,
  type Chapter,
  type Content,
  type Credit,
  type Item,
  type Tag,
  type TagSection,
} from "@suwatte/toolchain/types";
import { itemFrom } from "../_shared/item";
import { BASE_URL } from "./api";
import type { GalleryItem, Hentai, NHTag } from "./types";

const tagsOfType = (tags: NHTag[], type: string): NHTag[] =>
  tags.filter((tag) => tag.type === type);

const joinNames = (tags: NHTag[]): string =>
  tags.map((tag) => tag.name).join(", ");

const toTag = (tag: NHTag): Tag => ({
  id: `${tag.type}:${tag.name}`,
  title: tag.name,
  rating: ContentRating.MATURE,
});

export const itemFromGallery = (
  data: GalleryItem,
  thumbBase: string,
): Item => {
  const title = data.english_title || data.japanese_title || String(data.id);
  return itemFrom(
    String(data.id),
    title,
    `${thumbBase}/${data.thumbnail}`,
  );
};

export const contentFromHentai = (
  data: Hentai,
  thumbBase: string,
): Content => {
  const english = data.title.english ?? undefined;
  const japanese = data.title.japanese ?? undefined;
  const pretty = data.title.pretty ?? undefined;
  const title = english || japanese || pretty || String(data.id);

  const artists = tagsOfType(data.tags, "artist");
  const groups = tagsOfType(data.tags, "group");
  const categories = tagsOfType(data.tags, "category");
  const parodies = tagsOfType(data.tags, "parody");
  const characters = tagsOfType(data.tags, "character");
  const languages = tagsOfType(data.tags, "language");
  const genres = tagsOfType(data.tags, "tag").map(toTag);

  const credits: Credit[] = [
    ...artists.map((tag) => ({ name: tag.name, role: "Artist" })),
    ...groups.map((tag) => ({ name: tag.name, role: "Group" })),
  ];

  const properties: TagSection[] = [];
  if (categories.length) {
    properties.push({
      id: "categories",
      title: "Categories",
      tags: categories.map(toTag),
    });
  }
  if (parodies.length) {
    properties.push({
      id: "parodies",
      title: "Parodies",
      tags: parodies.map(toTag),
    });
  }
  if (characters.length) {
    properties.push({
      id: "characters",
      title: "Characters",
      tags: characters.map(toTag),
    });
  }
  if (languages.length) {
    properties.push({
      id: "languages",
      title: "Languages",
      tags: languages.map(toTag),
    });
  }
  if (artists.length) {
    properties.push({
      id: "artists",
      title: "Artists",
      tags: artists.map(toTag),
    });
  }
  if (groups.length) {
    properties.push({
      id: "groups",
      title: "Groups",
      tags: groups.map(toTag),
    });
  }

  const summaryParts = [
    "Full English and Japanese titles:",
    english || japanese || pretty || "",
    japanese && japanese !== english ? japanese : "",
    "",
    `Pages: ${data.num_pages}`,
    `Favorited by: ${data.num_favorites}`,
  ];
  if (categories.length) summaryParts.push(`Categories: ${joinNames(categories)}`);
  if (parodies.length) summaryParts.push(`Parodies: ${joinNames(parodies)}`);
  if (characters.length) summaryParts.push(`Characters: ${joinNames(characters)}`);

  const additionalTitles = [english, japanese, pretty].filter(
    (value): value is string => !!value && value !== title,
  );

  return {
    title,
    coverImage: `${thumbBase}/${data.thumbnail.path}`,
    webUrl: `${BASE_URL}/g/${data.id}/`,
    rating: ContentRating.MATURE,
    status: ContentStatus.COMPLETED,
    contentType: ContentType.COMIC,
    summary: summaryParts.filter((line) => line !== undefined).join("\n").trim(),
    additionalTitles: additionalTitles.length ? [...new Set(additionalTitles)] : undefined,
    additionalDetails: {
      Pages: String(data.num_pages),
      Favorites: String(data.num_favorites),
      ...(categories.length ? { Categories: joinNames(categories) } : {}),
      ...(parodies.length ? { Parodies: joinNames(parodies) } : {}),
    },
    statistics: {
      favorites: data.num_favorites,
    },
    credits: credits.length ? credits : undefined,
    genres: genres.length ? genres : undefined,
    properties: properties.length ? properties : undefined,
    characters: characters.length
      ? characters.map((tag) => ({ name: tag.name, role: "Character" }))
      : undefined,
  };
};

export const chapterFromHentai = (data: Hentai): Chapter => {
  const groups = joinNames(tagsOfType(data.tags, "group"));
  return {
    id: "1",
    index: 0,
    number: 1,
    title: "Chapter",
    date: new Date(data.upload_date * 1000),
    webUrl: `${BASE_URL}/g/${data.id}/`,
    providers: groups
      ? [{ id: "group", name: groups, links: [] }]
      : undefined,
  };
};
