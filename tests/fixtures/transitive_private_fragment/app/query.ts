const q = gql`
  query GetMe {
    me {
      ...PublicFragment
    }
  }
`;
