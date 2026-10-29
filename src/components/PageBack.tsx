import { useNavigate } from "react-router-dom";
import IconBtn from "./IconBtn";
import { IconBack } from "./icons";

/**
 * Borderless back-to-home icon button.
 * Sits at the right edge of a page's first row (page-header) — never on its own row.
 */
export default function PageBack() {
  const navigate = useNavigate();
  return (
    <IconBtn
      icon={<IconBack size={15} />}
      title="返回主界面"
      onClick={() => navigate("/")}
    />
  );
}
