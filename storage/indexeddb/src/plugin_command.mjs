// A transaction owns the slot and quota across all tabs. Resolve only after
// strict-durability commit; never send a financial request on a failed write.
export async function pluginCommandJournal(encoded) {
  const p = JSON.parse(encoded);
  const db = await new Promise((resolve, reject) => {
    const request = indexedDB.open("flare-plugin-commands-v1", 1);
    request.onupgradeneeded = () => {
      const store = request.result.createObjectStore("commands", {keyPath: "key"});
      store.createIndex("owner", "owner");
    };
    request.onerror = () => reject(new Error("journal storage unavailable"));
    request.onblocked = () => reject(new Error("journal storage blocked"));
    request.onsuccess = () => resolve(request.result);
  });
  return new Promise((resolve, reject) => {
    let tx;
    try { tx = db.transaction("commands", p.operation === "pending" ? "readonly" : "readwrite", {durability: "strict"}); }
    catch (e) { db.close(); reject(e); return; }
    const store = tx.objectStore("commands");
    const key = [p.tenantId, p.userId, p.slot];
    let result = null;
    const failure = error => { result = {error}; };
    tx.oncomplete = () => { db.close(); resolve(JSON.stringify(result)); };
    tx.onerror = tx.onabort = () => { db.close(); reject(new Error("journal transaction failed")); };
    const request = store.get(key);
    request.onsuccess = () => {
      const old = request.result;
      const record = row => ({command: JSON.parse(row.commandJson), response: row.responseJson === null ? null : JSON.parse(row.responseJson)});
      try {
        if (p.operation === "pending") { result = old ? record(old) : null; return; }
        if (p.operation === "prepare") {
          if (old) {
            if (old.commandJson !== p.commandJson) failure("pending_requires_reconciliation");
            else result = record(old);
            return;
          }
          const count = store.index("owner").count([p.tenantId, p.userId]);
          count.onsuccess = () => {
            if (count.result >= 32) { failure("slot_limit"); return; }
            const row = {key, owner: [p.tenantId, p.userId], commandJson: p.commandJson, responseJson: null};
            store.add(row); result = record(row);
          };
          return;
        }
        if (p.operation === "complete") {
          if (!old || old.commandJson !== p.commandJson) { failure("command_changed"); return; }
          if (old.responseJson === null) { old.responseJson = p.responseJson; store.put(old); }
          result = record(old); return;
        }
        if (p.operation === "acknowledge") {
          if (!old) return;
          if (record(old).command.commandId !== p.commandId || old.responseJson === null) { failure("unresolved_command"); return; }
          store.delete(key); return;
        }
        failure("invalid_command");
      } catch (_) { failure("corrupt_journal"); }
    };
  });
}
