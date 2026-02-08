import { graphql } from './graphql';

export const settingsQuery = graphql(`query GetSettings {
  settings {
    theme
    notifications
  }
}`);
