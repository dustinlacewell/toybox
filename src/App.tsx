import { LibraryView } from "./views/LibraryView";
import { Stack } from "@ldlework/toybox-sdk/ui";
import "./App.css";

function App() {
  return (
    <Stack grow style={{ height: "100vh" }}>
      <LibraryView />
    </Stack>
  );
}

export default App;
