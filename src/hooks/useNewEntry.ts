import { useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { newEntryId } from "../utils/id";

/// Opens a blank draft in the entry pane.
///
/// The id is generated here rather than at the first save so the draft has its
/// final URL from the moment it opens. `EntryPaneRoute` keys the pane on the
/// route id, so swapping a placeholder for a real id after the first autosave
/// would remount the pane mid-edit and throw the draft away.
///
/// Nothing is written until the first autosave, so a draft the user abandons
/// without typing never reaches the vault.
export function useNewEntry(): () => void {
  const navigate = useNavigate();
  return useCallback(() => {
    navigate(`/entries/${newEntryId()}`, { state: { draft: true } });
  }, [navigate]);
}
