import { create } from "zustand";
import type { PageKey, RangeKey, ShareTemplate } from "./types";

export type DataView = "overview" | "sessions";

interface UiState {
  page: PageKey;
  range: RangeKey;
  dataView: DataView;
  selectedSessionId?: string;
  shareTemplate?: ShareTemplate;
  setPage: (page: PageKey) => void;
  setRange: (range: RangeKey) => void;
  setDataView: (view: DataView) => void;
  selectSession: (id: string | undefined) => void;
  openSessions: (sessionId?: string) => void;
  closeSessions: () => void;
  openShare: (template?: ShareTemplate) => void;
}

export const useUiStore = create<UiState>((set) => ({
  page: "vcti",
  range: "90d",
  dataView: "overview",
  selectedSessionId: undefined,
  shareTemplate: undefined,
  setPage: (page) => set((state) => ({
    page,
    dataView: page === "data" ? state.dataView : "overview",
    selectedSessionId: page === "data" ? state.selectedSessionId : undefined,
  })),
  setRange: (range) => set({ range }),
  setDataView: (dataView) => set({ dataView }),
  selectSession: (selectedSessionId) => set({ selectedSessionId }),
  openSessions: (sessionId) => set({
    page: "data",
    dataView: "sessions",
    selectedSessionId: sessionId,
  }),
  closeSessions: () => set({ dataView: "overview", selectedSessionId: undefined }),
  openShare: (shareTemplate) => set({ page: "share", shareTemplate }),
}));
