export interface HeadingNavigationRequest {
  id: number;
  kind: "heading";
  path: string;
  headingText: string;
  level: number;
}

export interface TagNavigationRequest {
  id: number;
  kind: "tag";
  path: string;
  tagName: string;
}

export type EditorNavigationRequest = HeadingNavigationRequest | TagNavigationRequest;

let _nextNavigationId = 1;
let _request = $state<EditorNavigationRequest | null>(null);

export const editorNavigationStore = {
  get request(): EditorNavigationRequest | null {
    return _request;
  },

  requestHeading(path: string, headingText: string, level: number) {
    _request = {
      id: _nextNavigationId++,
      kind: "heading",
      path,
      headingText,
      level,
    };
  },

  requestTag(path: string, tagName: string) {
    _request = {
      id: _nextNavigationId++,
      kind: "tag",
      path,
      tagName,
    };
  },

  consume(id: number) {
    if (_request?.id === id) {
      _request = null;
    }
  },

  clear() {
    _request = null;
  },
};
