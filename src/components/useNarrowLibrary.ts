import { useEffect, useState } from "react";

export function useNarrowLibrary() {
  const [narrow,setNarrow]=useState(()=>window.matchMedia("(max-width: 1000px)").matches);
  useEffect(()=>{const query=window.matchMedia("(max-width: 1000px)");const update=()=>setNarrow(query.matches);query.addEventListener("change",update);update();return()=>query.removeEventListener("change",update);},[]);
  return narrow;
}
