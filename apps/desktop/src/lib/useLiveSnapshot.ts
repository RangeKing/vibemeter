import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import type { LiveSnapshot } from "../types";
import { api } from "./api";

export function useLiveSnapshot() {
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ["live-snapshot"],
    queryFn: api.liveSnapshot,
    refetchInterval: 1_500,
    staleTime: 400,
  });
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<LiveSnapshot>("live-update", (event) => {
      client.setQueryData(["live-snapshot"], event.payload);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [client]);
  return query;
}
