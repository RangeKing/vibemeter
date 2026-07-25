import { create } from "zustand";
import type { PageKey, RangeKey, ShareTemplate } from "./types";

interface UiState {
  page: PageKey;
  range: RangeKey;
  selectedSessionId?: string;
  shareTemplate?: ShareTemplate;
  setPage: (page: PageKey) => void;
  setRange: (range: RangeKey) => void;
  selectSession: (id: string | undefined) => void;
  openShare: (template?: ShareTemplate) => void;
}

export const useUiStore = create<UiState>((set) => ({
  page: "live",
  range: "30d",
  selectedSessionId: undefined,
  shareTemplate: undefined,
  setPage: (page) => set({ page }),
  setRange: (range) => set({ range }),
  selectSession: (selectedSessionId) => set({ selectedSessionId }),
  openShare: (shareTemplate) => set({ page: "share", shareTemplate }),
}));
