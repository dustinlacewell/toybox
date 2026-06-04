import { LibraryView } from "./views/LibraryView";
import { Stack } from "./ds/Stack";
import "./App.css";

function App() {
  return (
    <Stack grow style={{ height: "100vh" }}>
      <LibraryView />
    </Stack>
  );
}

export default App;
