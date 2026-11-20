import { Button, PageHeader } from "@ldlework/toybox-sdk/ui";

interface Props {
  title: string;
  visibleCount: number; visibleApproximate?: boolean; totalCount: number;
  onImport?: () => void;
}
/** The library's title line: what is shown, how many, and Import. The query and the filters sit under it. */
export function LibraryToolbar({ title, visibleCount, visibleApproximate, totalCount, onImport }: Props) {
  return <PageHeader title={title} metadata={visibleCount === totalCount && !visibleApproximate ? totalCount.toLocaleString() : `${visibleCount.toLocaleString()}${visibleApproximate ? "+" : ""} of ${totalCount.toLocaleString()}`}>
    {onImport && <Button variant="primary" onClick={onImport}>Import</Button>}
  </PageHeader>;
}
