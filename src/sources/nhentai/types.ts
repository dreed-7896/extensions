export type NHConfig = {
  image_servers: string[];
  thumb_servers: string[];
};

export type PaginatedResponse<T> = {
  result: T[];
  per_page: number;
  num_pages?: number | null;
  total?: number | null;
};

export type GalleryItem = {
  id: number;
  thumbnail: string;
  english_title?: string | null;
  japanese_title?: string | null;
};

export type NHTitle = {
  english?: string | null;
  japanese?: string | null;
  pretty?: string | null;
};

export type NHImage = {
  path: string;
};

export type NHTag = {
  name: string;
  type: string;
};

export type Hentai = {
  id: number;
  pages: NHImage[];
  thumbnail: NHImage;
  tags: NHTag[];
  title: NHTitle;
  upload_date: number;
  num_favorites: number;
  num_pages: number;
};
