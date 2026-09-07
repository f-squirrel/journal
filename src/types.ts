export interface EntryHeader {
  id: string;
  title: string;
  updatedAt: string;
}

export interface JournalEntry extends EntryHeader {
  markdown: string;
}
