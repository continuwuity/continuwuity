import { HomeLayout as BasicHomeLayout, DocContent } from "@rspress/core/theme-original";

import { useFrontmatter } from '@rspress/core/runtime';
interface HomeLayoutProps {
    components?: Record<string, React.FC>;
}

function HomeLayout(props: HomeLayoutProps) {
    console.log(props)

    const { frontmatter } = useFrontmatter();

    return (
        <BasicHomeLayout
            afterFeatures={
                (frontmatter.doc) ?
                <main className="rp-doc-layout__doc-container">
                    <div className="rp-doc rspress-doc">
                        <DocContent components={props.components} />
                    </div>
                </main>
                : <></>
            }
        />
    );
}
export { HomeLayout };
export * from "@rspress/core/theme-original";
import "./index.css";
