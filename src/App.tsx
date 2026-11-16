import { LibraryView } from "./views/LibraryView";
import { Stack } from "@ldlework/toybox-sdk/ui";
import "./App.css";
import { AppUpdateNotice } from "./components/AppUpdateNotice";

function App() {
  return (
    <Stack grow style={{ height: "100vh" }}>
      <LibraryView />
      <AppUpdateNotice/>
    </Stack>
  );
}

export default App;
