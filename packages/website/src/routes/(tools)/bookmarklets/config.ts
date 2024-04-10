export interface Config {
    author?:      string;
    description?: string;
    email?:       string;
    license?:     string;
    name?:        string;
    repository?:  string;
    script?:      string[];
    style?:       string[];
    url?:         string;
    version?:     string;
    [x: string]: any;
}
