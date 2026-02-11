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
            beforeFeatures={
                frontmatter.beforeFeatures ? (
                    <section className="custom-section">
                        <div className="rp-container">
                            <div className="custom-cards">
                                {frontmatter.beforeFeatures.map((item: any, index: number) => (
                                    <a key={index} href={item.link} className="custom-card" target="_blank" rel="noopener noreferrer">
                                        <h3>{item.title}</h3>
                                        <p>{item.details}</p>
                                        <span className="custom-card-button">{item.buttonText || 'Learn More'} →</span>
                                    </a>
                                ))}
                            </div>
                        </div>
                    </section>
                ) : <></>
            }
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
