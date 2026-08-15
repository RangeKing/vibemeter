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
  openSessions: (sessionId?: string) => void;
  openShare: (template?: ShareTemplate) => void;
}

export const useUiStore = create<UiState>((set) => ({
  page: "data",
  range: "90d",
  selectedSessionId: undefined,
  shareTemplate: undefined,
  setPage: (page) => set((state) => ({
    page,
    selectedSessionId: page === "sessions" ? state.selectedSessionId : undefined,
  })),
  setRange: (range) => set({ range }),
  selectSession: (selectedSessionId) => set({ selectedSessionId }),
  openSessions: (sessionId) => set({
    page: "sessions",
    selectedSessionId: sessionId,
  }),
  openShare: (shareTemplate) => set({ page: "share", shareTemplate }),
}));
