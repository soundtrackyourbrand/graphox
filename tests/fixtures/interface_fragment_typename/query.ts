const q = gql`
  fragment DisplayableInfo on Displayable {
    display {
      title
    }
  }

  query TestQuery {
    items {
      ...DisplayableInfo
    }
  }
`;
